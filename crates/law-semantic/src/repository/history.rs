//! L3 historyStructure of `history.yaml` (loj-history/v1alpha1: HIST, REV;
//! common: TIME) and its evidence references (TYPE-03, TYPE-05).
use super::{EVIDENCE as EV, HISTORY, HS};
use crate::common::findings::{common_type_rule, Findings};
use crate::common::time::{calc, fixed_day, Date};
use crate::common::{Check, Class, COMMON};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub const SCHEMA: &str =
    include_str!("../../../../vendor/loj-spec/schemas/loj-history/v1alpha1/history.schema.json");
pub const TYPES: &str =
    include_str!("../../../../vendor/loj-spec/schemas/common/v1alpha1/types.schema.json");

pub(crate) fn validator() -> &'static jsonschema::Validator {
    static V: std::sync::OnceLock<jsonschema::Validator> = std::sync::OnceLock::new();
    V.get_or_init(|| crate::common::validator(SCHEMA, &[TYPES]))
}

const FILE: &str = "history.yaml";

/// Map a schema error of the history manifest to its rule.
pub(crate) fn classify(schema_path: &str, _instance: &str) -> (Check, &'static str, &'static str) {
    if schema_path.contains("/$defs/revisionId") {
        return (HS, "REV-01", HISTORY);
    }
    if let Some((check, rule)) = common_type_rule(schema_path) {
        return (check, rule, COMMON);
    }
    (HS, "HIST-01", HISTORY)
}

/// Walk every TimePoint (`effective`, `base`) with its JSON pointer.
pub(crate) fn time_points<'a>(v: &'a Value, pointer: String, out: &mut Vec<(String, &'a Value)>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                let p = format!("{pointer}/{k}");
                if (k == "effective" || k == "base") && x.is_object() {
                    out.push((p.clone(), x));
                }
                time_points(x, p, out);
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                time_points(x, format!("{pointer}/{i}"), out);
            }
        }
        _ => {}
    }
}

/// Walk every Evidence list (`evidence`, `surveyed`, `text.sources`).
pub(crate) fn evidence_refs<'a>(v: &'a Value, pointer: String, out: &mut Vec<(String, &'a str)>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                let p = format!("{pointer}/{k}");
                let list = k == "evidence"
                    || k == "surveyed"
                    || (k == "sources" && m.contains_key("status"));
                if list {
                    if let Some(a) = x.as_array() {
                        for (i, e) in a.iter().enumerate() {
                            if let Some(s) = e["source"].as_str() {
                                out.push((format!("{p}/{i}"), s));
                            }
                        }
                    }
                }
                evidence_refs(x, p, out);
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                evidence_refs(x, format!("{pointer}/{i}"), out);
            }
        }
        _ => {}
    }
}

/// TIME-02 / TIME-03 / TIME-04 on one TimePoint.
pub(crate) fn check_time(tp: &Value, pointer: &str, file: &str, f: &mut Findings, check: Check) {
    let value = tp["value"].as_str();
    match tp["state"].as_str() {
        Some("date") => {
            let ok = match (tp["precision"].as_str(), value) {
                (Some("day"), Some(v)) => Date::parse(v).is_some(),
                (Some("month"), Some(v)) => {
                    v.len() == 7 && Date::parse(&format!("{v}-01")).is_some()
                }
                (Some("year"), Some(v)) => {
                    v.len() == 4 && Date::parse(&format!("{v}-01-01")).is_some()
                }
                _ => true, // shape errors are TIME-01 (schema)
            };
            if !ok {
                f.error(
                    check,
                    "TIME-02",
                    COMMON,
                    "date value is not an existing Gregorian date of its precision",
                    file,
                    pointer,
                );
            }
        }
        Some("derived") => {
            if let Some(base) = fixed_day(&tp["base"]) {
                match value.and_then(Date::parse) {
                    None => f.error(
                        check,
                        "TIME-03",
                        COMMON,
                        "derived with a fixed base requires the computed value",
                        file,
                        pointer,
                    ),
                    Some(v) => {
                        if tp.get("calc").is_some() {
                            match calc(&tp["calc"], base) {
                                Some(c) if c == v => {}
                                Some(c) => f.error(
                                    check,
                                    "TIME-03",
                                    COMMON,
                                    format!(
                                        "value {} differs from calc result {}",
                                        v.to_iso(),
                                        c.to_iso()
                                    ),
                                    file,
                                    pointer,
                                ),
                                None => {}
                            }
                        } else {
                            f.note(
                                check,
                                Class::Undetermined,
                                "TIME-03",
                                COMMON,
                                "derived without calc: computed value not checked",
                                file,
                                pointer,
                            );
                        }
                    }
                }
            }
        }
        _ => {}
    }
    if tp.get("determinedBy").is_some() && tp.get("delegation").is_none() {
        f.error(
            check,
            "TIME-04",
            COMMON,
            "determinedBy requires the delegation it determined",
            file,
            pointer,
        );
    }
}

/// The elements of an array, or nothing when `v` is not an array (the schema
/// check has already reported that).
pub(crate) fn list(v: &Value) -> &[Value] {
    v.as_array().map_or(&[], Vec::as_slice)
}

/// Revisions and relations of the manifest by id.
type ById<'a> = HashMap<&'a str, &'a Value>;

/// Check one parsed history manifest (schemaVersion already confirmed).
pub(crate) fn check(h: &Value, repo: &Path, f: &mut Findings) {
    f.applicable.insert(HS);
    f.applicable.insert(EV);
    f.schema(validator(), h, FILE, classify);
    let sources = source_ids(h, f);
    evidence_sources(h, &sources, f);
    let relations = relation_ids(h, f);
    let revisions = list(&h["revisions"]);
    let by_id = revision_ids(revisions, f);
    // HIST-04: origins, basedOn targets and cycles.
    if !revisions.iter().any(|r| r.get("origin").is_some()) {
        f.error(
            HS,
            "HIST-04",
            HISTORY,
            "the manifest has no origin revision",
            FILE,
            "/revisions",
        );
    }
    for (i, r) in revisions.iter().enumerate() {
        let p = format!("/revisions/{i}");
        based_on(r, &p, &by_id, f);
        basis(r, &p, &relations, f);
        replaced_by(r, &p, &by_id, f);
    }
    one_revision_per_time(revisions, f);
    packages(revisions, &by_id, repo, f);
    termination(h, &relations, revisions, f);
    // TIME-02..04 on every TimePoint.
    let mut tps = Vec::new();
    time_points(h, String::new(), &mut tps);
    for (p, tp) in tps {
        check_time(tp, &p, FILE, f, HS);
    }
}

/// HIST-02: Source ids are unique.
fn source_ids<'a>(h: &'a Value, f: &mut Findings) -> HashSet<&'a str> {
    let mut sources = HashSet::new();
    for (i, s) in list(&h["sources"]).iter().enumerate() {
        if let Some(id) = s["id"].as_str() {
            if !sources.insert(id) {
                f.error(
                    HS,
                    "HIST-02",
                    HISTORY,
                    format!("duplicate Source id {id}"),
                    FILE,
                    &format!("/sources/{i}"),
                );
            }
        }
    }
    sources
}

/// TYPE-03: Evidence points to a Source of this file.
fn evidence_sources(h: &Value, sources: &HashSet<&str>, f: &mut Findings) {
    let mut refs = Vec::new();
    evidence_refs(h, String::new(), &mut refs);
    for (p, s) in refs {
        if !sources.contains(s) {
            f.error(
                EV,
                "TYPE-03",
                COMMON,
                format!("Evidence source {s} is not a Source of this file"),
                FILE,
                &p,
            );
        }
    }
}

/// HIST-03: relation ids, and item ids within a relation, are unique.
fn relation_ids<'a>(h: &'a Value, f: &mut Findings) -> ById<'a> {
    let mut relations = ById::new();
    for (i, r) in list(&h["relations"]).iter().enumerate() {
        let Some(id) = r["id"].as_str() else { continue };
        if relations.insert(id, r).is_some() {
            f.error(
                HS,
                "HIST-03",
                HISTORY,
                format!("duplicate relation id {id}"),
                FILE,
                &format!("/relations/{i}"),
            );
        }
        let mut items = HashSet::new();
        for (j, it) in list(&r["items"]).iter().enumerate() {
            if let Some(iid) = it["id"].as_str() {
                if !items.insert(iid) {
                    f.error(
                        HS,
                        "HIST-03",
                        HISTORY,
                        format!("duplicate item id {iid} in relation {id}"),
                        FILE,
                        &format!("/relations/{i}/items/{j}"),
                    );
                }
            }
        }
    }
    relations
}

/// REV-01: revision ids are unique.
fn revision_ids<'a>(revisions: &'a [Value], f: &mut Findings) -> ById<'a> {
    let mut by_id = ById::new();
    for (i, r) in revisions.iter().enumerate() {
        if let Some(id) = r["id"].as_str() {
            if by_id.insert(id, r).is_some() {
                f.error(
                    HS,
                    "REV-01",
                    HISTORY,
                    format!("duplicate revision_id {id}"),
                    FILE,
                    &format!("/revisions/{i}/id"),
                );
            }
        }
    }
    by_id
}

/// HIST-04: the basedOn target exists and the chain has no cycle.
/// HIST-06: the base state does not take effect after the revision.
fn based_on(r: &Value, p: &str, by_id: &ById, f: &mut Findings) {
    let b = match &r["basedOn"] {
        Value::String(b) => b.as_str(),
        Value::Object(_) => {
            f.note(
                HS,
                Class::Pending,
                "HIST-06",
                HISTORY,
                "basedOn is unknown; order pending",
                FILE,
                &format!("{p}/basedOn"),
            );
            return;
        }
        _ => return,
    };
    let Some(base) = by_id.get(b) else {
        f.error(
            HS,
            "HIST-04",
            HISTORY,
            format!("basedOn {b} is not a revision of this manifest"),
            FILE,
            &format!("{p}/basedOn"),
        );
        return;
    };
    let mut seen = HashSet::from([r["id"].as_str().unwrap_or("")]);
    let mut cur = b;
    while let Some(next) = by_id.get(cur) {
        if !seen.insert(cur) {
            f.error(
                HS,
                "HIST-04",
                HISTORY,
                "basedOn forms a cycle",
                FILE,
                &format!("{p}/basedOn"),
            );
            break;
        }
        match next["basedOn"].as_str() {
            Some(n) => cur = n,
            None => break,
        }
    }
    let day = |t: &Value| (t["state"] == "date").then(|| fixed_day(t)).flatten();
    match (day(&base["effective"]), day(&r["effective"])) {
        (Some(a), Some(c)) if a > c => f.note(
            HS,
            Class::Warning,
            "HIST-06",
            HISTORY,
            "base state takes effect after this revision",
            FILE,
            &format!("{p}/effective"),
        ),
        (Some(_), Some(_)) => {}
        _ => f.note(
            HS,
            Class::Undetermined,
            "HIST-06",
            HISTORY,
            "effective times are not both dates; order not determined",
            FILE,
            &format!("{p}/effective"),
        ),
    }
}

/// HIST-05: basis refers to items of one relation of this manifest, and
/// each item takes effect when the revision does.
fn basis(r: &Value, p: &str, relations: &ById, f: &mut Findings) {
    let mut basis_relations = HashSet::new();
    for (j, b) in list(&r["basis"]).iter().enumerate() {
        let Some(rel) = b["relation"].as_str() else {
            continue;
        };
        basis_relations.insert(rel);
        let bp = format!("{p}/basis/{j}");
        let Some(relation) = relations.get(rel) else {
            f.error(
                HS,
                "HIST-05",
                HISTORY,
                format!("basis relation {rel} is not in this manifest"),
                FILE,
                &bp,
            );
            continue;
        };
        for (k, item) in list(&b["items"]).iter().enumerate() {
            let Some(iid) = item.as_str() else { continue };
            match list(&relation["items"]).iter().find(|x| x["id"] == iid) {
                None => f.error(
                    HS,
                    "HIST-05",
                    HISTORY,
                    format!("basis item {iid} is not an item of relation {rel}"),
                    FILE,
                    &format!("{bp}/items/{k}"),
                ),
                Some(x) => {
                    let (a, c) = (&x["effective"], &r["effective"]);
                    if a["state"] == "date"
                        && c["state"] == "date"
                        && (a["value"] != c["value"] || a["precision"] != c["precision"])
                    {
                        f.error(
                            HS,
                            "HIST-05",
                            HISTORY,
                            format!("item {iid} takes effect at another time than the revision"),
                            FILE,
                            &format!("{p}/effective"),
                        );
                    }
                }
            }
        }
    }
    if basis_relations.len() > 1 {
        f.error(
            HS,
            "HIST-05",
            HISTORY,
            "basis refers to more than one relation",
            FILE,
            &format!("{p}/basis"),
        );
    }
}

/// HIST-10: replacedBy targets exist.
fn replaced_by(r: &Value, p: &str, by_id: &ById, f: &mut Findings) {
    for (j, t) in list(&r["replacedBy"]["revisions"]).iter().enumerate() {
        if let Some(t) = t.as_str() {
            if !by_id.contains_key(t) {
                f.error(
                    HS,
                    "HIST-10",
                    HISTORY,
                    format!("replacedBy {t} is not a revision of this manifest"),
                    FILE,
                    &format!("{p}/replacedBy/revisions/{j}"),
                );
            }
        }
    }
}

/// HIST-05: one active revision per relation and effective time.
fn one_revision_per_time(revisions: &[Value], f: &mut Findings) {
    let mut seen = HashMap::new();
    for (i, r) in revisions.iter().enumerate() {
        if r["status"] != "active" {
            continue;
        }
        for b in list(&r["basis"]) {
            if let Some(rel) = b["relation"].as_str() {
                let key = (rel, r["effective"].to_string());
                if let Some(first) = seen.insert(key, i) {
                    f.error(
                        HS,
                        "HIST-05",
                        HISTORY,
                        format!(
                            "revisions {first} and {i} share relation {rel} and effective time"
                        ),
                        FILE,
                        &format!("/revisions/{i}"),
                    );
                }
            }
        }
    }
}

/// HIST-08 / REV-03: packages and version directories correspond one-to-one.
fn packages(revisions: &[Value], by_id: &ById, repo: &Path, f: &mut Findings) {
    let mut expected_dirs = HashSet::new();
    for (i, r) in revisions.iter().enumerate() {
        let Some(id) = r["id"].as_str() else { continue };
        let p = format!("/revisions/{i}/text");
        let status = r["text"]["status"].as_str().unwrap_or("");
        if matches!(status, "complete" | "partial") {
            let package = package_path(r);
            if let Some(d) = Path::new(&package).parent() {
                expected_dirs.insert(d.to_string_lossy().replace('\\', "/"));
            }
            package_files(r, id, &package, &p, repo, f);
        } else if status == "absent" && repo.join("versions").join(id).exists() {
            f.error(
                HS,
                "HIST-08",
                HISTORY,
                format!("revision {id} has no text but versions/{id} exists"),
                FILE,
                &p,
            );
        }
    }
    let Ok(entries) = std::fs::read_dir(repo.join("versions")) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let rel = format!("versions/{name}");
        if !expected_dirs.contains(&rel)
            && by_id
                .get(name.as_str())
                .is_none_or(|r| r["text"]["status"] != "absent")
        {
            f.error(
                HS,
                "HIST-08",
                HISTORY,
                format!("{rel} has no revision with text"),
                FILE,
                "/revisions",
            );
        }
    }
}

/// REV-03 / HIST-08 for one revision that has text: the package directory is
/// named by the revision id, and the package and its glyph notes exist.
fn package_files(r: &Value, id: &str, package: &str, p: &str, repo: &Path, f: &mut Findings) {
    let dir = Path::new(package)
        .parent()
        .and_then(|d| d.file_name())
        .and_then(|d| d.to_str());
    if dir != Some(id) {
        f.error(
            HS,
            "REV-03",
            HISTORY,
            format!("package directory of {id} is not named by its revision_id"),
            FILE,
            &format!("{p}/package"),
        );
    }
    if !repo.join(package).is_file() {
        f.error(
            HS,
            "HIST-08",
            HISTORY,
            format!("package {package} of revision {id} does not exist"),
            FILE,
            p,
        );
    }
    let default_notes = format!("versions/{id}/glyphs.yaml");
    match r["text"]["glyphNotes"].as_str() {
        Some(n) if !repo.join(n).is_file() => f.error(
            HS,
            "HIST-08",
            HISTORY,
            format!("glyphNotes {n} does not exist"),
            FILE,
            &format!("{p}/glyphNotes"),
        ),
        None if repo.join(&default_notes).is_file() => f.error(
            HS,
            "HIST-08",
            HISTORY,
            format!("{default_notes} exists but text.glyphNotes is missing"),
            FILE,
            p,
        ),
        _ => {}
    }
}

/// HIST-12: termination refers to a relation of the matching type, and no
/// revision takes effect after it.
fn termination(h: &Value, relations: &ById, revisions: &[Value], f: &mut Findings) {
    for (i, t) in list(&h["termination"]).iter().enumerate() {
        let p = format!("/termination/{i}");
        let want = match t["type"].as_str() {
            Some("repealed") => "repeals",
            Some("totallyRevised") => "totallyRevises",
            _ => continue,
        };
        let Some(rel) = t["relation"].as_str() else {
            continue;
        };
        let Some(r) = relations.get(rel) else {
            f.error(
                HS,
                "HIST-12",
                HISTORY,
                format!("termination relation {rel} is not in this manifest"),
                FILE,
                &p,
            );
            continue;
        };
        if r["type"] != want {
            f.error(
                HS,
                "HIST-12",
                HISTORY,
                format!(
                    "termination {} needs a {want} relation, {rel} is {}",
                    t["type"], r["type"]
                ),
                FILE,
                &p,
            );
            continue;
        }
        let end = list(&r["items"])
            .iter()
            .find_map(|x| fixed_day(&x["effective"]));
        let Some(end) = end else { continue };
        for (j, rv) in revisions.iter().enumerate() {
            if fixed_day(&rv["effective"]).is_some_and(|d| d > end) {
                f.note(
                    HS,
                    Class::Warning,
                    "HIST-12",
                    HISTORY,
                    "revision takes effect after the termination",
                    FILE,
                    &format!("/revisions/{j}/effective"),
                );
            }
        }
    }
}

/// `text.package`, or `versions/<revision_id>/law.yaml`.
pub(crate) fn package_path(r: &Value) -> String {
    r["text"]["package"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("versions/{}/law.yaml", r["id"].as_str().unwrap_or("")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn found(f: &Findings) -> Vec<(&str, Class, String)> {
        f.items
            .iter()
            .map(|(_, d)| {
                let pointer = d.location.as_ref().unwrap()["pointer"].as_str().unwrap();
                (d.rule.as_str(), d.class, pointer.to_owned())
            })
            .collect()
    }

    fn day(value: &str) -> Value {
        json!({"state": "date", "precision": "day", "value": value})
    }

    #[test]
    fn based_on_reports_missing_targets_cycles_and_order() {
        let revisions = json!([
            {"id": "a", "basedOn": "b", "effective": day("2020-01-01")},
            {"id": "b", "basedOn": "a", "effective": day("2021-01-01")},
            {"id": "c", "basedOn": "x"},
            {"id": "d", "basedOn": {"state": "unknown"}},
            {"id": "e"},
            {"id": "f", "basedOn": "e", "effective": {"state": "date", "precision": "month", "value": "2020-01"}},
            {"id": "g", "basedOn": "a", "effective": day("2020-01-01")},
        ]);
        let revisions = list(&revisions);
        let mut f = Findings::default();
        let by_id = revision_ids(revisions, &mut f);
        assert!(f.items.is_empty());
        for (i, r) in revisions.iter().enumerate() {
            based_on(r, &format!("/revisions/{i}"), &by_id, &mut f);
        }
        assert_eq!(
            found(&f),
            [
                // a -> b -> a, and b (2021) takes effect after a (2020).
                ("HIST-04", Class::Error, "/revisions/0/basedOn".to_owned()),
                (
                    "HIST-06",
                    Class::Warning,
                    "/revisions/0/effective".to_owned()
                ),
                // b -> a -> b; a is not after b.
                ("HIST-04", Class::Error, "/revisions/1/basedOn".to_owned()),
                ("HIST-04", Class::Error, "/revisions/2/basedOn".to_owned()),
                ("HIST-06", Class::Pending, "/revisions/3/basedOn".to_owned()),
                // A month is not a fixed day: the order is not decided.
                (
                    "HIST-06",
                    Class::Undetermined,
                    "/revisions/5/effective".to_owned()
                ),
                // g -> a -> b -> a: the cycle is reached, not started, by g.
                ("HIST-04", Class::Error, "/revisions/6/basedOn".to_owned()),
            ]
        );
    }

    #[test]
    fn duplicate_ids_are_reported_where_they_repeat() {
        let h = json!({
            "sources": [{"id": "s"}, {"id": "s"}, {}],
            "relations": [
                {"id": "r", "items": [{"id": "i"}, {"id": "i"}]},
                {"id": "r"},
            ],
            "revisions": [{"id": "v"}, {"id": "v"}, {}],
        });
        let mut f = Findings::default();
        assert_eq!(source_ids(&h, &mut f).len(), 1);
        assert_eq!(relation_ids(&h, &mut f).len(), 1);
        assert_eq!(revision_ids(list(&h["revisions"]), &mut f).len(), 1);
        assert_eq!(
            found(&f),
            [
                ("HIST-02", Class::Error, "/sources/1".to_owned()),
                ("HIST-03", Class::Error, "/relations/0/items/1".to_owned()),
                ("HIST-03", Class::Error, "/relations/1".to_owned()),
                ("REV-01", Class::Error, "/revisions/1/id".to_owned()),
            ]
        );
        // A value that is not an array has no elements; the schema reports it.
        assert!(list(&json!({"a": 1})).is_empty());
        assert!(list(&Value::Null).is_empty());
    }

    #[test]
    fn basis_must_name_items_of_one_relation_at_the_same_time() {
        let h = json!({"relations": [
            {"id": "r1", "items": [
                {"id": "i1", "effective": day("2020-04-01")},
                {"id": "i2", "effective": day("2021-04-01")},
            ]},
            {"id": "r2", "items": []},
        ]});
        let mut f = Findings::default();
        let relations = relation_ids(&h, &mut f);
        let ok = json!({"effective": day("2020-04-01"),
            "basis": [{"relation": "r1", "items": ["i1"]}]});
        basis(&ok, "/revisions/0", &relations, &mut f);
        assert!(f.items.is_empty());
        let bad = json!({"effective": day("2020-04-01"), "basis": [
            {"relation": "r1", "items": ["i2", "zz"]},
            {"relation": "nope", "items": ["i1"]},
            {"relation": "r2", "items": []},
        ]});
        basis(&bad, "/revisions/1", &relations, &mut f);
        assert_eq!(
            found(&f),
            [
                ("HIST-05", Class::Error, "/revisions/1/effective".to_owned()),
                (
                    "HIST-05",
                    Class::Error,
                    "/revisions/1/basis/0/items/1".to_owned()
                ),
                ("HIST-05", Class::Error, "/revisions/1/basis/1".to_owned()),
                ("HIST-05", Class::Error, "/revisions/1/basis".to_owned()),
            ]
        );
    }

    #[test]
    fn termination_needs_a_relation_of_its_type() {
        let h = json!({
            "relations": [
                {"id": "rep", "type": "repeals", "items": [{"effective": day("2022-01-01")}]},
                {"id": "amd", "type": "amends"},
            ],
            "termination": [
                {"type": "repealed", "relation": "rep"},
                {"type": "repealed", "relation": "amd"},
                {"type": "totallyRevised", "relation": "gone"},
                {"type": "expired", "relation": "gone"},
            ],
        });
        let revisions = json!([
            {"effective": day("2021-12-31")},
            {"effective": day("2022-01-02")},
        ]);
        let mut f = Findings::default();
        let relations = relation_ids(&h, &mut f);
        termination(&h, &relations, list(&revisions), &mut f);
        assert_eq!(
            found(&f),
            [
                (
                    "HIST-12",
                    Class::Warning,
                    "/revisions/1/effective".to_owned()
                ),
                ("HIST-12", Class::Error, "/termination/1".to_owned()),
                ("HIST-12", Class::Error, "/termination/2".to_owned()),
            ]
        );
    }
}
