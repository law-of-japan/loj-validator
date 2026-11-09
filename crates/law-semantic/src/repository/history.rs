//! L3 historyStructure of `history.yaml` (loj-history/v1alpha1: HIST, REV;
//! common: TIME) and its evidence references (TYPE-03, TYPE-05).
use super::{EVIDENCE as EV, HISTORY, HS};
use crate::common::findings::{common_type_rule, Findings};
use crate::common::time::{calc, fixed_day, Date};
use crate::common::{Check, COMMON};
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
                                "undetermined",
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

/// Check one parsed history manifest (schemaVersion already confirmed).
pub(crate) fn check(h: &Value, repo: &Path, f: &mut Findings) {
    f.applicable.insert(HS);
    f.applicable.insert(EV);
    f.schema(validator(), h, FILE, classify);
    let empty = Vec::new();
    let arr = |v: &Value| v.as_array().unwrap_or(&empty).clone();
    // HIST-02: Source ids are unique.
    let mut sources = HashSet::new();
    for (i, s) in arr(&h["sources"]).iter().enumerate() {
        if let Some(id) = s["id"].as_str() {
            if !sources.insert(id.to_owned()) {
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
    // TYPE-03: Evidence points to a Source of this file.
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
    // HIST-03: relation ids and item ids.
    let mut relations: HashMap<String, Value> = HashMap::new();
    for (i, r) in arr(&h["relations"]).iter().enumerate() {
        let Some(id) = r["id"].as_str() else { continue };
        if relations.insert(id.to_owned(), r.clone()).is_some() {
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
        for (j, it) in arr(&r["items"]).iter().enumerate() {
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
    let revisions = arr(&h["revisions"]);
    let mut by_id: HashMap<String, &Value> = HashMap::new();
    for (i, r) in revisions.iter().enumerate() {
        if let Some(id) = r["id"].as_str() {
            if by_id.insert(id.to_owned(), r).is_some() {
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
        match &r["basedOn"] {
            Value::String(b) => {
                if !by_id.contains_key(b) {
                    f.error(
                        HS,
                        "HIST-04",
                        HISTORY,
                        format!("basedOn {b} is not a revision of this manifest"),
                        FILE,
                        &format!("{p}/basedOn"),
                    );
                } else {
                    let mut seen = HashSet::from([r["id"].as_str().unwrap_or("")]);
                    let mut cur = b.as_str();
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
                    // HIST-06: order of effective times.
                    let base = by_id[b];
                    let day = |t: &Value| (t["state"] == "date").then(|| fixed_day(t)).flatten();
                    match (day(&base["effective"]), day(&r["effective"])) {
                        (Some(a), Some(c)) if a > c => f.note(
                            HS,
                            "warning",
                            "HIST-06",
                            HISTORY,
                            "base state takes effect after this revision",
                            FILE,
                            &format!("{p}/effective"),
                        ),
                        (Some(_), Some(_)) => {}
                        _ => f.note(
                            HS,
                            "undetermined",
                            "HIST-06",
                            HISTORY,
                            "effective times are not both dates; order not determined",
                            FILE,
                            &format!("{p}/effective"),
                        ),
                    }
                }
            }
            Value::Object(_) => f.note(
                HS,
                "pending",
                "HIST-06",
                HISTORY,
                "basedOn is unknown; order pending",
                FILE,
                &format!("{p}/basedOn"),
            ),
            _ => {}
        }
        // HIST-05: basis refers to items of one relation of this manifest.
        let mut basis_relations = HashSet::new();
        for (j, b) in arr(&r["basis"]).iter().enumerate() {
            let Some(rel) = b["relation"].as_str() else {
                continue;
            };
            basis_relations.insert(rel.to_owned());
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
            for (k, item) in arr(&b["items"]).iter().enumerate() {
                let Some(iid) = item.as_str() else { continue };
                match arr(&relation["items"]).iter().find(|x| x["id"] == iid) {
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
                                format!(
                                    "item {iid} takes effect at another time than the revision"
                                ),
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
        // HIST-10: replacedBy targets exist.
        for (j, t) in arr(&r["replacedBy"]["revisions"]).iter().enumerate() {
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
    // HIST-05: one revision per relation and effective time.
    let mut seen = HashMap::new();
    for (i, r) in revisions.iter().enumerate() {
        if r["status"] != "active" {
            continue;
        }
        for b in arr(&r["basis"]) {
            if let Some(rel) = b["relation"].as_str() {
                let key = (rel.to_owned(), r["effective"].to_string());
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
    // HIST-08 / REV-03: packages and version directories correspond one-to-one.
    let mut expected_dirs = HashSet::new();
    for (i, r) in revisions.iter().enumerate() {
        let Some(id) = r["id"].as_str() else { continue };
        let p = format!("/revisions/{i}/text");
        let status = r["text"]["status"].as_str().unwrap_or("");
        let package = package_path(r);
        if matches!(status, "complete" | "partial") {
            let dir = Path::new(&package)
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
            if let Some(d) = Path::new(&package).parent() {
                expected_dirs.insert(d.to_string_lossy().replace('\\', "/"));
            }
            if !repo.join(&package).is_file() {
                f.error(
                    HS,
                    "HIST-08",
                    HISTORY,
                    format!("package {package} of revision {id} does not exist"),
                    FILE,
                    &p,
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
                    &p,
                ),
                _ => {}
            }
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
    if let Ok(entries) = std::fs::read_dir(repo.join("versions")) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let rel = format!("versions/{name}");
            if !expected_dirs.contains(&rel)
                && by_id
                    .get(&name)
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
    // HIST-12: termination refers to a relation of the matching type.
    for (i, t) in arr(&h["termination"]).iter().enumerate() {
        let p = format!("/termination/{i}");
        let want = match t["type"].as_str() {
            Some("repealed") => "repeals",
            Some("totallyRevised") => "totallyRevises",
            _ => continue,
        };
        let Some(rel) = t["relation"].as_str() else {
            continue;
        };
        match relations.get(rel) {
            None => f.error(
                HS,
                "HIST-12",
                HISTORY,
                format!("termination relation {rel} is not in this manifest"),
                FILE,
                &p,
            ),
            Some(r) if r["type"] != want => f.error(
                HS,
                "HIST-12",
                HISTORY,
                format!(
                    "termination {} needs a {want} relation, {rel} is {}",
                    t["type"], r["type"]
                ),
                FILE,
                &p,
            ),
            Some(r) => {
                if let Some(end) = arr(&r["items"])
                    .iter()
                    .find_map(|x| fixed_day(&x["effective"]))
                {
                    for (j, rv) in revisions.iter().enumerate() {
                        if fixed_day(&rv["effective"]).is_some_and(|d| d > end) {
                            f.note(
                                HS,
                                "warning",
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
        }
    }
    // TIME-02..04 on every TimePoint.
    let mut tps = Vec::new();
    time_points(h, String::new(), &mut tps);
    for (p, tp) in tps {
        check_time(tp, &p, FILE, f, HS);
    }
}

/// `text.package`, or `versions/<revision_id>/law.yaml`.
pub(crate) fn package_path(r: &Value) -> String {
    r["text"]["package"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("versions/{}/law.yaml", r["id"].as_str().unwrap_or("")))
}
