//! L4 (common/v1alpha1 XREF-01..03): relationResolution, relationConsistency
//! and timeConsistency over the registry and several law repositories.
use crate::common::findings::Findings;
use crate::common::time::fixed_day;
use crate::common::{Check, Class, Report, COMMON};
use crate::registry::{self, Resolution};
use crate::repository::history::list;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

const RES: Check = Check::RELATION_RESOLUTION;
const CON: Check = Check::RELATION_CONSISTENCY;
const TIME: Check = Check::TIME_CONSISTENCY;

pub fn check(registry_dir: &Path, repositories: &[std::path::PathBuf], checks: &[Check]) -> Report {
    let mut f = Findings::default();
    let mut references: Vec<(Check, Value)> = Vec::new();
    let mut scratch = Findings::default();
    let reg = registry::load(registry_dir, &mut scratch);
    // Manifests by Record ID; unreadable ones leave the L4 results incomplete.
    let mut manifests: BTreeMap<String, (String, Value)> = BTreeMap::new();
    for repo in repositories {
        let mut hf = Findings::default();
        match crate::repository::load_history(repo, &mut hf) {
            Some(h) => {
                if let Some(id) = h["record"]["id"].as_str() {
                    manifests.insert(id.to_owned(), (repo.to_string_lossy().into_owned(), h));
                }
            }
            None => {
                for c in [RES, CON, TIME] {
                    f.incomplete.insert(c);
                }
                f.items.extend(hf.items);
            }
        }
    }
    let Some(reg) = reg else {
        for c in [RES, CON, TIME] {
            f.incomplete.insert(c);
        }
        f.items.extend(scratch.items);
        return finish(f, references, checks);
    };
    for c in [RES, CON, TIME] {
        f.applicable.insert(c);
    }
    for (this, (path, h)) in &manifests {
        let subject = Subject {
            reg: &reg,
            manifests: &manifests,
            this,
            file: format!("{path}/history.yaml"),
            h,
        };
        subject.endpoints(&mut f, &mut references);
        subject.total_revision_relations(&mut f);
        subject.total_revision_origins(&mut f);
        subject.same_as(&mut f, &mut references);
    }
    finish(f, references, checks)
}

/// One history manifest under the L4 checks, and what it is checked against.
struct Subject<'a> {
    reg: &'a registry::Registry,
    /// Histories by Record ID: repository path and manifest.
    manifests: &'a BTreeMap<String, (String, Value)>,
    /// Record ID of this manifest.
    this: &'a str,
    /// Path of this manifest, for diagnostics.
    file: String,
    h: &'a Value,
}

impl Subject<'_> {
    /// A `references` entry of the report for a reference from this Record.
    fn reference(&self, target: &str, result: &str) -> Value {
        json!({"from": format!("{}/{}", self.reg.space, self.this), "target": target, "result": result})
    }

    /// XREF-01: this Record and the endpoints of its relations are registered.
    fn endpoints(&self, f: &mut Findings, references: &mut Vec<(Check, Value)>) {
        let (reg, file, h) = (self.reg, &self.file, self.h);
        // The end point is this Record itself; it must be registered.
        let me = &h["record"];
        if !matches!(reg.resolve(me), Resolution::Resolved(_)) {
            f.incomplete.insert(RES);
            f.note(
                RES,
                Class::Undetermined,
                "XREF-01",
                COMMON,
                format!("{} is not resolved in the registry", reg.target(me)),
                file,
                "/record",
            );
        }
        let mut endpoints = Vec::new();
        for (i, r) in list(&h["relations"]).iter().enumerate() {
            let confirmed = r["status"] == "active" && r["verification"]["state"] == "confirmed";
            endpoints.push((format!("/relations/{i}/from"), &r["from"], confirmed));
        }
        for (i, r) in list(&h["revisions"]).iter().enumerate() {
            if r["totallyRevises"].is_object() {
                endpoints.push((
                    format!("/revisions/{i}/totallyRevises/record"),
                    &r["totallyRevises"]["record"],
                    true,
                ));
            }
        }
        for (ptr, rref, confirmed) in endpoints {
            let res = reg.resolve(rref);
            references.push((RES, self.reference(&reg.target(rref), res.as_str())));
            match res {
                Resolution::Resolved(_) => {}
                Resolution::NotFetched(why) => {
                    f.incomplete.insert(RES);
                    f.note(
                        RES,
                        Class::Undetermined,
                        "XREF-01",
                        COMMON,
                        format!("{} not fetched: {why}", reg.target(rref)),
                        file,
                        &ptr,
                    );
                }
                Resolution::NonexistentConfirmed => {
                    if confirmed {
                        f.error(
                            RES,
                            "XREF-01",
                            COMMON,
                            format!(
                                "confirmed relation refers to {} confirmed nonexistent",
                                reg.target(rref)
                            ),
                            file,
                            &ptr,
                        );
                    }
                }
            }
        }
    }

    /// The history of the Record that `record` refers to. When it cannot be
    /// examined, relationConsistency is incomplete; a history that was not
    /// given is noted at `ptr`.
    fn counterpart(&self, record: &Value, ptr: &str, f: &mut Findings) -> Option<(String, &Value)> {
        let Some(id) = resolved_id(self.reg, record) else {
            f.incomplete.insert(CON);
            return None;
        };
        let Some((_, other)) = self.manifests.get(&id) else {
            f.incomplete.insert(CON);
            f.note(
                CON,
                Class::Undetermined,
                "XREF-02",
                COMMON,
                format!("history of {id} not given"),
                &self.file,
                ptr,
            );
            return None;
        };
        Some((id, other))
    }

    /// XREF-02: an active totallyRevises relation has a totalRevision origin
    /// in the history of the Record it comes from.
    fn total_revision_relations(&self, f: &mut Findings) {
        for (i, r) in list(&self.h["relations"]).iter().enumerate() {
            if r["type"] != "totallyRevises" || r["status"] != "active" {
                continue;
            }
            let ptr = format!("/relations/{i}");
            let Some((from, other)) = self.counterpart(&r["from"], &ptr, f) else {
                continue;
            };
            let agrees = list(&other["revisions"]).iter().any(|v| {
                v["origin"] == "totalRevision"
                    && resolved_id(self.reg, &v["totallyRevises"]["record"]).as_deref()
                        == Some(self.this)
                    && v["totallyRevises"]["relation"] == r["id"]
            });
            if !agrees {
                f.error(
                    CON,
                    "XREF-02",
                    COMMON,
                    format!(
                        "{from} has no totalRevision origin for relation {}",
                        r["id"]
                    ),
                    &self.file,
                    &ptr,
                );
            }
        }
    }

    /// XREF-02: a totalRevision origin names a totallyRevises relation, from
    /// this Record, in the history of the Record it revises.
    fn total_revision_origins(&self, f: &mut Findings) {
        for (i, v) in list(&self.h["revisions"]).iter().enumerate() {
            if v["origin"] != "totalRevision" {
                continue;
            }
            let ptr = format!("/revisions/{i}/totallyRevises");
            let Some((old, other)) = self.counterpart(&v["totallyRevises"]["record"], &ptr, f)
            else {
                continue;
            };
            let rel = list(&other["relations"])
                .iter()
                .find(|r| r["id"] == v["totallyRevises"]["relation"]);
            let ok = rel.is_some_and(|r| {
                r["type"] == "totallyRevises"
                    && resolved_id(self.reg, &r["from"]).as_deref() == Some(self.this)
            });
            if !ok {
                f.error(
                    CON,
                    "XREF-02",
                    COMMON,
                    format!(
                        "{old} has no totallyRevises relation {} from {}",
                        v["totallyRevises"]["relation"], self.this
                    ),
                    &self.file,
                    &ptr,
                );
            }
        }
    }

    /// XREF-03: sameAs points to a Revision with the same time.
    fn same_as(&self, f: &mut Findings, references: &mut Vec<(Check, Value)>) {
        let (reg, file) = (self.reg, &self.file);
        let mut tps = Vec::new();
        crate::repository::history::time_points(self.h, String::new(), &mut tps);
        for (ptr, tp) in tps {
            let s = &tp["sameAs"];
            if !s.is_object() {
                continue;
            }
            let rev = s["revision"].as_str().unwrap_or("");
            let target = format!("{}/{rev}", reg.target(&s["record"]));
            let other = match reg.resolve(&s["record"]) {
                Resolution::Resolved(id) => self.manifests.get(&id),
                _ => None,
            };
            let Some((_, other)) = other else {
                references.push((TIME, self.reference(&target, "notFetched")));
                f.incomplete.insert(TIME);
                f.note(
                    TIME,
                    Class::Undetermined,
                    "XREF-03",
                    COMMON,
                    format!("{target} not fetched"),
                    file,
                    &ptr,
                );
                continue;
            };
            let Some(r) = list(&other["revisions"]).iter().find(|r| r["id"] == rev) else {
                references.push((TIME, self.reference(&target, "nonexistentConfirmed")));
                f.error(
                    TIME,
                    "XREF-03",
                    COMMON,
                    format!("{target}: no such revision"),
                    file,
                    &ptr,
                );
                continue;
            };
            references.push((TIME, self.reference(&target, "resolved")));
            match (fixed_day(tp), fixed_day(&r["effective"])) {
                (Some(a), Some(b)) if a != b => f.error(
                    TIME,
                    "XREF-03",
                    COMMON,
                    format!(
                        "{} differs from {target} effective {}",
                        a.to_iso(),
                        b.to_iso()
                    ),
                    file,
                    &ptr,
                ),
                (Some(_), Some(_)) => {}
                _ => f.note(
                    TIME,
                    Class::Undetermined,
                    "XREF-03",
                    COMMON,
                    format!("{target}: not both fixed dates"),
                    file,
                    &ptr,
                ),
            }
        }
    }
}

fn resolved_id(reg: &registry::Registry, r: &Value) -> Option<String> {
    match reg.resolve(r) {
        Resolution::Resolved(id) => Some(id),
        _ => None,
    }
}

fn finish(f: Findings, references: Vec<(Check, Value)>, checks: &[Check]) -> Report {
    let mut seen = Vec::new();
    for (c, r) in references {
        if checks.contains(&c) && !seen.contains(&r) {
            seen.push(r);
        }
    }
    let mut report = f.into_report(checks);
    report.references = seen;
    report
}
