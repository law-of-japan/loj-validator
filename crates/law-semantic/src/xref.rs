//! L4 (common/v1alpha1 XREF-01..03): relationResolution, relationConsistency
//! and timeConsistency over the registry and several law repositories.
use crate::common::findings::Findings;
use crate::common::time::fixed_day;
use crate::common::{Check, Class, Report, COMMON};
use crate::registry::{self, Resolution};
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
    let empty = Vec::new();
    for (this, (path, h)) in &manifests {
        let file = format!("{path}/history.yaml");
        let me = &h["record"];
        // XREF-01: endpoints of every relation.
        // The end point is this Record itself; it must be registered.
        if !matches!(reg.resolve(me), Resolution::Resolved(_)) {
            f.incomplete.insert(RES);
            f.note(
                RES,
                Class::Undetermined,
                "XREF-01",
                COMMON,
                format!("{} is not resolved in the registry", reg.target(me)),
                &file,
                "/record",
            );
        }
        let mut endpoints = Vec::new();
        for (i, r) in h["relations"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .enumerate()
        {
            let confirmed = r["status"] == "active" && r["verification"]["state"] == "confirmed";
            endpoints.push((format!("/relations/{i}/from"), r["from"].clone(), confirmed));
        }
        for (i, r) in h["revisions"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .enumerate()
        {
            if r["totallyRevises"].is_object() {
                endpoints.push((
                    format!("/revisions/{i}/totallyRevises/record"),
                    r["totallyRevises"]["record"].clone(),
                    true,
                ));
            }
        }
        for (ptr, rref, confirmed) in endpoints {
            let res = reg.resolve(&rref);
            references.push((RES, json!({"from": format!("{}/{this}", reg.space), "target": reg.target(&rref), "result": res.as_str()})));
            match res {
                Resolution::Resolved(_) => {}
                Resolution::NotFetched(why) => {
                    f.incomplete.insert(RES);
                    f.note(
                        RES,
                        Class::Undetermined,
                        "XREF-01",
                        COMMON,
                        format!("{} not fetched: {why}", reg.target(&rref)),
                        &file,
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
                                reg.target(&rref)
                            ),
                            &file,
                            &ptr,
                        );
                    }
                }
            }
        }
        // XREF-02: totallyRevises relations and totalRevision origins agree.
        for (i, r) in h["relations"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .enumerate()
        {
            if r["type"] != "totallyRevises" || r["status"] != "active" {
                continue;
            }
            let ptr = format!("/relations/{i}");
            let Some(from) = resolved_id(&reg, &r["from"]) else {
                f.incomplete.insert(CON);
                continue;
            };
            let Some((_, other)) = manifests.get(&from) else {
                f.incomplete.insert(CON);
                f.note(
                    CON,
                    Class::Undetermined,
                    "XREF-02",
                    COMMON,
                    format!("history of {from} not given"),
                    &file,
                    &ptr,
                );
                continue;
            };
            let agrees = other["revisions"]
                .as_array()
                .unwrap_or(&empty)
                .iter()
                .any(|v| {
                    v["origin"] == "totalRevision"
                        && resolved_id(&reg, &v["totallyRevises"]["record"]).as_deref()
                            == Some(this)
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
                    &file,
                    &ptr,
                );
            }
        }
        for (i, v) in h["revisions"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .enumerate()
        {
            if v["origin"] != "totalRevision" {
                continue;
            }
            let ptr = format!("/revisions/{i}/totallyRevises");
            let Some(old) = resolved_id(&reg, &v["totallyRevises"]["record"]) else {
                f.incomplete.insert(CON);
                continue;
            };
            let Some((_, other)) = manifests.get(&old) else {
                f.incomplete.insert(CON);
                f.note(
                    CON,
                    Class::Undetermined,
                    "XREF-02",
                    COMMON,
                    format!("history of {old} not given"),
                    &file,
                    &ptr,
                );
                continue;
            };
            let rel = other["relations"]
                .as_array()
                .unwrap_or(&empty)
                .iter()
                .find(|r| r["id"] == v["totallyRevises"]["relation"]);
            let ok = rel.is_some_and(|r| {
                r["type"] == "totallyRevises"
                    && resolved_id(&reg, &r["from"]).as_deref() == Some(this)
            });
            if !ok {
                f.error(
                    CON,
                    "XREF-02",
                    COMMON,
                    format!(
                        "{old} has no totallyRevises relation {} from {this}",
                        v["totallyRevises"]["relation"]
                    ),
                    &file,
                    &ptr,
                );
            }
        }
        // XREF-03: sameAs points to a Revision with the same time.
        let mut tps = Vec::new();
        crate::repository::history::time_points(h, String::new(), &mut tps);
        for (ptr, tp) in tps {
            let s = &tp["sameAs"];
            if !s.is_object() {
                continue;
            }
            let rev = s["revision"].as_str().unwrap_or("");
            let target = format!("{}/{rev}", reg.target(&s["record"]));
            let other = match reg.resolve(&s["record"]) {
                Resolution::Resolved(id) => manifests.get(&id),
                _ => None,
            };
            let Some((_, other)) = other else {
                references.push((TIME, json!({"from": format!("{}/{this}", reg.space), "target": target, "result": "notFetched"})));
                f.incomplete.insert(TIME);
                f.note(
                    TIME,
                    Class::Undetermined,
                    "XREF-03",
                    COMMON,
                    format!("{target} not fetched"),
                    &file,
                    &ptr,
                );
                continue;
            };
            let Some(r) = other["revisions"]
                .as_array()
                .unwrap_or(&empty)
                .iter()
                .find(|r| r["id"] == rev)
            else {
                references.push((TIME, json!({"from": format!("{}/{this}", reg.space), "target": target, "result": "nonexistentConfirmed"})));
                f.error(
                    TIME,
                    "XREF-03",
                    COMMON,
                    format!("{target}: no such revision"),
                    &file,
                    &ptr,
                );
                continue;
            };
            references.push((TIME, json!({"from": format!("{}/{this}", reg.space), "target": target, "result": "resolved"})));
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
                    &file,
                    &ptr,
                ),
                (Some(_), Some(_)) => {}
                _ => f.note(
                    TIME,
                    Class::Undetermined,
                    "XREF-03",
                    COMMON,
                    format!("{target}: not both fixed dates"),
                    &file,
                    &ptr,
                ),
            }
        }
    }
    finish(f, references, checks)
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
