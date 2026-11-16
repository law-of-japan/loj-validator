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
const DISPLAY: Check = Check::DISPLAY_CONSISTENCY;

pub fn check_with_space(
    registry_dir: &Path,
    repositories: &[std::path::PathBuf],
    checks: &[Check],
    space: Option<&str>,
) -> Report {
    let mut f = Findings::default();
    let mut references: Vec<(Check, Value)> = Vec::new();
    let mut scratch = Findings::default();
    let reg = registry::load_with_space(registry_dir, space, &mut scratch);
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
                for c in [RES, CON, TIME, DISPLAY] {
                    f.incomplete.insert(c);
                }
                f.items.extend(hf.items);
            }
        }
    }
    let Some(reg) = reg else {
        for c in [RES, CON, TIME, DISPLAY] {
            f.incomplete.insert(c);
        }
        f.items.extend(scratch.items);
        return finish(f, references, checks);
    };
    for c in [RES, CON, TIME, DISPLAY] {
        f.applicable.insert(c);
    }
    let mut per_record = Vec::new();
    for (this, (path, h)) in &manifests {
        let mut local = Findings::default();
        local.applicable.extend([RES, CON, TIME, DISPLAY]);
        let subject = Subject {
            reg: &reg,
            manifests: &manifests,
            this,
            file: format!("{path}/history.yaml"),
            h,
        };
        subject.endpoints(&mut local, &mut references);
        subject.total_revision_origins(&mut local);
        subject.same_as(&mut local, &mut references);
        if checks.contains(&DISPLAY) {
            display_numbers(&reg, Path::new(path), h, &mut local);
        }
        per_record.push((this, path, local));
    }
    let mut cross = Findings::default();
    promulgated_consistency(&reg, &manifests, &mut cross);
    let mut records = Vec::new();
    for (id, path, mut local) in per_record {
        local.items.extend(
            cross
                .items
                .iter()
                .filter(|(_, d)| {
                    d.location
                        .as_ref()
                        .is_some_and(|l| l["file"] == format!("{path}/history.yaml"))
                })
                .cloned(),
        );
        f.incomplete.extend(local.incomplete.iter().copied());
        f.items.extend(local.items.iter().cloned());
        let mut report = local.into_report(checks);
        report.target.insert("repository".into(), json!(path));
        records.push((id.clone(), report));
    }
    let mut report = finish(f, references, checks);
    report
        .target
        .insert("repository".into(), json!(registry_dir.to_string_lossy()));
    report.records = records;
    report
}

/// XREF-05 reads only the declared revisions' own default package paths.
fn display_numbers(reg: &registry::Registry, repo: &Path, history: &Value, f: &mut Findings) {
    let Some(id) = resolved_id(reg, &history["record"]) else {
        f.incomplete.insert(DISPLAY);
        return;
    };
    let Some(record) = reg.records.get(&id) else {
        return;
    };
    let Some(claim) = list(&record["numbers"])
        .iter()
        .find(|n| n["decision"] == "adopted")
    else {
        return;
    };
    if claim["state"] == "conflicting" {
        return;
    }
    let Ok(mut paths) = crate::common::documents::DocumentPaths::new(&repo.join("history.yaml"))
    else {
        f.incomplete.insert(DISPLAY);
        return;
    };
    let mut budget =
        crate::common::budget::ReadBudget::new(128 * 1024 * 1024, crate::common::MAX_FILE_BYTES);
    for revision in list(&history["revisions"]) {
        if !matches!(
            revision["text"]["status"].as_str(),
            Some("complete" | "partial")
        ) {
            continue;
        }
        let Some(id) = revision["id"].as_str() else {
            continue;
        };
        if id.len() != 8
            || !id
                .bytes()
                .all(|b| b"0123456789abcdefghjkmnpqrstvwxyz".contains(&b))
        {
            f.incomplete.insert(DISPLAY);
            continue;
        }
        let path = format!("versions/{id}/law.yaml");
        let manifest = paths
            .resolve_yaml(&path)
            .ok()
            .and_then(|p| budget.read_file(&p).ok().flatten())
            .and_then(|bytes| crate::common::strict_text(bytes).ok())
            .and_then(|text| crate::common::yaml::parse(&text).ok());
        let Some(manifest) = manifest.filter(|m| m["schemaVersion"] == "loj/v1alpha2") else {
            f.note(
                DISPLAY,
                Class::Undetermined,
                "XREF-05",
                COMMON,
                "package could not be read for display comparison",
                &path,
                "/law/number",
            );
            continue;
        };
        let number = &manifest["law"]["number"];
        let matches = match claim["state"].as_str() {
            Some("exists" | "illegible") => {
                number == &claim["raw"]
                    || claim["structured"]["period"]["raw"]
                        .as_str()
                        .zip(claim["raw"].as_str())
                        .is_some_and(|(period, raw)| {
                            number.as_str() == Some(format!("{period}{raw}").as_str())
                        })
            }
            Some("nonexistent" | "unknown") => number.is_null(),
            _ => continue,
        };
        if !matches {
            f.note(
                DISPLAY,
                Class::Warning,
                "XREF-05",
                COMMON,
                "law.number differs from the adopted number claim",
                &path,
                "/law/number",
            );
        }
    }
}

/// XREF-06 groups by the resolved origin and compares only across Records.
fn promulgated_consistency(
    reg: &registry::Registry,
    manifests: &BTreeMap<String, (String, Value)>,
    f: &mut Findings,
) {
    use crate::common::time::Date;
    use std::collections::BTreeSet;
    let mut groups: BTreeMap<String, BTreeMap<&str, BTreeSet<Option<Date>>>> = BTreeMap::new();
    for (record, (_, history)) in manifests {
        for relation in list(&history["relations"]) {
            if relation["status"] == "retracted" || !relation["promulgated"].is_object() {
                continue;
            }
            let Some(origin) = resolved_id(reg, &relation["from"]) else {
                continue;
            };
            groups
                .entry(origin)
                .or_default()
                .entry(record)
                .or_default()
                .insert(fixed_day(&relation["promulgated"]));
        }
    }
    for (origin, records) in groups {
        if records.len() < 2 {
            continue;
        }
        for record in records.keys() {
            let file = format!("{}/history.yaml", manifests[*record].0);
            if records.values().any(|days| days.contains(&None)) {
                f.note(
                    TIME,
                    Class::Undetermined,
                    "XREF-06",
                    COMMON,
                    format!("promulgation of {origin} is not fixed in all Records"),
                    &file,
                    "/relations",
                );
            }
            let fixed_records = records
                .values()
                .filter(|days| days.iter().any(Option::is_some))
                .count();
            let days: BTreeSet<Date> = records
                .values()
                .flat_map(|days| days.iter().flatten().copied())
                .collect();
            if fixed_records >= 2 && days.len() > 1 {
                f.error(
                    TIME,
                    "XREF-06",
                    COMMON,
                    format!("promulgation of {origin} differs across Records"),
                    &file,
                    "/relations",
                );
            }
        }
    }
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
        let mut value = json!({"target": target, "result": result});
        if !self.reg.space.is_empty() {
            value["from"] = json!(format!("loj:{}/{}", self.reg.space, self.this));
        }
        value
    }

    /// XREF-01: this Record and the endpoints of its relations are registered.
    fn endpoints(&self, f: &mut Findings, references: &mut Vec<(Check, Value)>) {
        let (reg, file, h) = (self.reg, &self.file, self.h);
        let mut endpoints = Vec::new();
        for (i, r) in list(&h["relations"]).iter().enumerate() {
            let confirmed = r["status"] == "active" && r["verification"]["state"] == "confirmed";
            endpoints.push((format!("/relations/{i}/from"), &r["from"], confirmed));
            endpoints.push(("/record".into(), &h["record"], confirmed));
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
            if let Some(target) = reg.target(rref) {
                references.push((RES, self.reference(&target, res.as_str())));
            } else {
                f.note(
                    RES,
                    Class::Undetermined,
                    "XREF-01",
                    COMMON,
                    "registration space is needed to serialize the reference",
                    file,
                    &ptr,
                );
            }
            match res {
                Resolution::Resolved(_) => {}
                Resolution::NotFetched(why) => {
                    f.incomplete.insert(RES);
                    f.note(
                        RES,
                        Class::Undetermined,
                        "XREF-01",
                        COMMON,
                        format!("{rref} not fetched: {why}"),
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
                            format!("confirmed relation refers to {rref} confirmed nonexistent"),
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
            let own = resolved_id(self.reg, &self.h["record"]);
            if rel.is_some_and(|r| resolved_id(self.reg, &r["from"]).is_none()) || own.is_none() {
                f.note(
                    CON,
                    Class::Undetermined,
                    "XREF-02",
                    COMMON,
                    "relation endpoint is not fetched",
                    &self.file,
                    &ptr,
                );
                continue;
            }
            let ok = rel.is_some_and(|r| {
                r["type"] == "totallyRevises"
                    && r["status"] == "active"
                    && resolved_id(self.reg, &r["from"]) == own
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
            let reference_target = reg
                .target(&s["record"])
                .map(|target| format!("{target}/{rev}"));
            if reference_target.is_none() {
                f.note(
                    TIME,
                    Class::Undetermined,
                    "XREF-03",
                    COMMON,
                    "registration space is needed to serialize the reference",
                    file,
                    &ptr,
                );
            }
            let target = reference_target
                .as_deref()
                .unwrap_or("reference without a registration space");
            let other = match reg.resolve(&s["record"]) {
                Resolution::Resolved(id) => self.manifests.get(&id),
                _ => None,
            };
            let Some((_, other)) = other else {
                if reference_target.is_some() {
                    references.push((TIME, self.reference(target, "notFetched")));
                }
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
                if reference_target.is_some() {
                    references.push((TIME, self.reference(target, "nonexistentConfirmed")));
                }
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
            if reference_target.is_some() {
                references.push((TIME, self.reference(target, "resolved")));
            }
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
