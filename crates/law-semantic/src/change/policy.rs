//! CHG-02 file and metadata boundaries, independent of AST equality.
use super::{Change, Findings, Snapshot, CH, COMMON};
use crate::common::Class;
use crate::repository::history::list;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

fn indexed(values: &Value) -> BTreeMap<&str, &Value> {
    list(values)
        .iter()
        .filter_map(|v| v["id"].as_str().map(|id| (id, v)))
        .collect()
}

pub(super) fn revision_ids(snapshot: &Snapshot) -> BTreeSet<String> {
    snapshot
        .history
        .as_ref()
        .map(|h| {
            indexed(&h["revisions"])
                .keys()
                .map(|s| (*s).to_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn version_file(path: &str) -> Option<(&str, &str)> {
    path.strip_prefix("versions/")?.split_once('/')
}

fn sources(value: &Value, out: &mut BTreeSet<String>) {
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::Object(object) => {
                if let Some(source) = object.get("source").and_then(Value::as_str) {
                    out.insert(source.into());
                }
                pending.extend(object.values());
            }
            Value::Array(values) => pending.extend(values),
            _ => {}
        }
    }
}

/// Ambiguous IDs cannot be projected into maps for a change comparison. This
/// is a missing comparison prerequisite, not a duplicate L3 conformance check.
pub(super) fn processable(change: &Change<'_>, snapshot: &Snapshot, f: &mut Findings) -> bool {
    let Some(history) = &snapshot.history else {
        return true;
    };
    for field in ["revisions", "relations", "sources"] {
        let Some(value) = history.get(field) else {
            continue;
        };
        let mut seen = BTreeSet::new();
        let valid = value.as_array().is_some_and(|values| {
            values.iter().all(|v| {
                v["id"]
                    .as_str()
                    .is_some_and(|id| !id.is_empty() && seen.insert(id))
            })
        });
        if !valid {
            f.note(
                CH,
                Class::Undetermined,
                "CHG-02",
                COMMON,
                format!("cannot compare ambiguous or malformed {field} IDs"),
                change.at,
                "",
            );
            return false;
        }
    }
    true
}

fn remove(value: &mut Value, keys: &[&str]) {
    if let Some(object) = value.as_object_mut() {
        for key in keys {
            object.remove(*key);
        }
    }
}

pub(super) fn check(change: &Change<'_>, before: &Snapshot, after: &Snapshot, f: &mut Findings) {
    let old_ids = revision_ids(before);
    let new_ids = revision_ids(after);
    let affected: BTreeSet<String> = if change.kind == "revisionAdd" {
        new_ids.difference(&old_ids).cloned().collect()
    } else {
        change.declared.iter().cloned().collect()
    };
    if change.kind != "revisionAdd"
        && new_ids
            .difference(&old_ids)
            .any(|id| !affected.contains(id))
    {
        change.error(f, "CHG-02", "new Revision was not declared".into());
    }
    if change.kind == "revisionFix"
        && before
            .packages
            .values()
            .any(|path| before.files.contains_key(path) && !after.files.contains_key(path))
    {
        change.error(
            f,
            "CHG-02",
            "revisionFix must retain existing version packages".into(),
        );
    }
    if !old_ids.is_subset(&new_ids) {
        change.error(
            f,
            "CHG-02",
            "Revision records must be retained, including on revert".into(),
        );
    }
    if !matches!(change.kind, "revisionAdd" | "revisionFix") && old_ids != new_ids {
        change.error(
            f,
            "CHG-02",
            "This change kind cannot add or remove Revision identities".into(),
        );
    }
    let changed: BTreeSet<&String> = before
        .files
        .keys()
        .chain(after.files.keys())
        .filter(|path| before.files.get(*path) != after.files.get(*path))
        .collect();
    for path in changed {
        let version = version_file(path);
        let package = version.is_some_and(|(_, file)| file != "glyphs.yaml");
        let canonical = version.is_some()
            || matches!(path.as_str(), "history.yaml" | "rebuild.yaml")
            || path.starts_with("records/");
        if !canonical {
            // CHG-01 excludes generators, CI configuration and documentation:
            // only the canonical data kinds are subject to change classification.
            continue;
        }
        let allowed = match change.kind {
            "format" => package,
            "metadata" => !package,
            "correction" | "officialCorrection" | "restoration" | "revisionAdd" | "revisionFix" => {
                match version {
                    Some((id, _)) => affected.contains(id),
                    None if path == "history.yaml" => true,
                    None if path == "rebuild.yaml" => {
                        matches!(change.kind, "restoration" | "revisionAdd")
                    }
                    _ => false,
                }
            }
            _ => false,
        };
        if !allowed {
            change.error(f, "CHG-02", format!("{} cannot change {path}", change.kind));
        }
        if canonical
            && path.ends_with(".yaml")
            && after.files.contains_key(path)
            && !after.documents.contains_key(path)
        {
            f.note(
                CH,
                Class::Undetermined,
                "CHG-02",
                COMMON,
                format!("cannot inspect changed YAML {path}"),
                change.at,
                "",
            );
        }
    }
    if matches!(
        change.kind,
        "correction" | "officialCorrection" | "restoration" | "revisionAdd" | "revisionFix"
    ) {
        history(change, before, after, &affected, f);
        if before.files.get("rebuild.yaml") != after.files.get("rebuild.yaml") {
            mappings(change, before, after, &affected, f);
        }
    }
    if change.kind == "metadata" {
        registry_ids(change, before, after, f);
    }
}

fn history(
    change: &Change<'_>,
    before: &Snapshot,
    after: &Snapshot,
    affected: &BTreeSet<String>,
    f: &mut Findings,
) {
    if before.history.is_none() && after.history.is_some() && change.kind != "revisionAdd" {
        change.error(
            f,
            "CHG-02",
            "only revisionAdd can introduce a history manifest in a text change".into(),
        );
    }
    let empty = serde_json::json!({});
    let (old, new) = (
        before.history.as_ref().unwrap_or(&empty),
        after.history.as_ref().unwrap_or(&empty),
    );
    let (old_revisions, new_revisions) = (indexed(&old["revisions"]), indexed(&new["revisions"]));
    let mut allowed_sources = BTreeSet::new();
    let mut selected_items: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for h in [old, new] {
        for revision in list(&h["revisions"])
            .iter()
            .filter(|r| r["id"].as_str().is_some_and(|id| affected.contains(id)))
        {
            sources(revision, &mut allowed_sources);
            for basis in list(&revision["basis"]) {
                if let Some(relation) = basis["relation"].as_str() {
                    selected_items.entry(relation.into()).or_default().extend(
                        list(&basis["items"])
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned),
                    );
                }
            }
        }
    }
    for (id, revision) in &old_revisions {
        let Some(next) = new_revisions.get(id) else {
            continue;
        };
        let (mut a, mut b) = ((*revision).clone(), (*next).clone());
        if affected.contains(*id) && change.kind != "revisionAdd" {
            if change.kind == "revisionFix" {
                continue;
            }
            let fields: &[&str] = if change.kind == "restoration" {
                &["status", "gaps", "sources", "glyphCheck", "glyphNotes"]
            } else {
                &["sources", "glyphCheck", "glyphNotes"]
            };
            if let Some(text) = a.get_mut("text") {
                remove(text, fields);
            }
            if let Some(text) = b.get_mut("text") {
                remove(text, fields);
            }
        }
        if a != b {
            change.error(
                f,
                "CHG-02",
                format!("unrelated Revision metadata changed: {id}"),
            );
        }
    }
    if matches!(
        change.kind,
        "correction" | "officialCorrection" | "restoration"
    ) {
        for id in affected {
            let Some(revision) = new_revisions.get(id.as_str()) else {
                change.error(
                    f,
                    "CHG-02",
                    format!("declared Revision does not exist: {id}"),
                );
                continue;
            };
            if list(&revision["text"]["sources"]).is_empty() {
                change.error(
                    f,
                    "CHG-02",
                    format!("declared Revision {id} requires source evidence"),
                );
            }
            if change.kind == "officialCorrection"
                && !list(&revision["text"]["sources"]).iter().any(|e| {
                    list(&new["sources"])
                        .iter()
                        .any(|s| s["id"] == e["source"] && s["kind"] == "officialCorrection")
                })
            {
                change.error(
                    f,
                    "CHG-02",
                    format!("Revision {id} has no official-correction Source"),
                );
            }
            if change.kind == "restoration"
                && old_revisions.get(id.as_str()).is_some_and(|old| {
                    old["text"]["status"] == revision["text"]["status"]
                        && old["text"]["gaps"] == revision["text"]["gaps"]
                })
            {
                change.error(
                    f,
                    "CHG-02",
                    format!("restoration does not update the text status or gaps of {id}"),
                );
            }
        }
    }
    let (old_relations, new_relations) = (indexed(&old["relations"]), indexed(&new["relations"]));
    for id in old_relations
        .keys()
        .chain(new_relations.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        let (a, b) = (old_relations.get(id), new_relations.get(id));
        if a == b {
            continue;
        }
        if a.is_none() && change.kind == "revisionAdd" {
            if let Some(relation) = b {
                sources(relation, &mut allowed_sources);
            }
            continue;
        }
        let permitted = selected_items.get(id);
        if change.kind == "revisionFix" && permitted.is_some() {
            for relation in [a, b].into_iter().flatten() {
                sources(relation, &mut allowed_sources);
            }
            continue;
        }
        let project = |relation: Option<&&Value>| {
            let mut value = relation.map_or(Value::Null, |value| (*value).clone());
            if let Some(items) = permitted {
                remove(&mut value, &["evidence"]);
                if let Some(values) = value.get_mut("items").and_then(Value::as_array_mut) {
                    for item in values {
                        if item["id"].as_str().is_some_and(|id| items.contains(id)) {
                            remove(item, &["evidence"]);
                            if let Some(scope) = item.get_mut("scope") {
                                remove(scope, &["nodes"]);
                            }
                        }
                    }
                }
            }
            value
        };
        if change.kind == "revisionAdd" || project(a) != project(b) {
            change.error(
                f,
                "CHG-02",
                format!("unrelated relation metadata changed: {id}"),
            );
        } else {
            for relation in [a, b].into_iter().flatten() {
                sources(relation, &mut allowed_sources);
            }
        }
    }
    let (old_sources, new_sources) = (indexed(&old["sources"]), indexed(&new["sources"]));
    for id in old_sources
        .keys()
        .chain(new_sources.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        if old_sources.get(id) == new_sources.get(id) {
            continue;
        }
        if !allowed_sources.contains(id)
            || (change.kind == "revisionAdd" && old_sources.contains_key(id))
        {
            change.error(f, "CHG-02", format!("unrelated Source changed: {id}"));
        }
    }
    let (mut a, mut b) = (old.clone(), new.clone());
    remove(&mut a, &["revisions", "relations", "sources"]);
    remove(&mut b, &["revisions", "relations", "sources"]);
    if change.kind == "revisionFix" {
        for value in [&mut a, &mut b] {
            if let Some(termination) = value.get_mut("termination").and_then(Value::as_array_mut) {
                termination.retain(|t| {
                    !t["relation"]
                        .as_str()
                        .is_some_and(|id| selected_items.contains_key(id))
                });
                if termination.is_empty() {
                    remove(value, &["termination"]);
                }
            }
        }
    }
    if before.history.is_some() && a != b {
        change.error(f, "CHG-02", "unrelated history metadata changed".into());
    }
}

fn mappings(
    change: &Change<'_>,
    before: &Snapshot,
    after: &Snapshot,
    affected: &BTreeSet<String>,
    f: &mut Findings,
) {
    let project = |snapshot: &Snapshot| {
        let mut doc = snapshot
            .documents
            .get("rebuild.yaml")
            .cloned()
            .unwrap_or(Value::Null);
        if let Some(mappings) = doc.get_mut("mappings").and_then(Value::as_array_mut) {
            mappings.retain_mut(|mapping| {
                let Some(revisions) = mapping.get_mut("revisions").and_then(Value::as_array_mut)
                else {
                    return true;
                };
                let had_revisions = !revisions.is_empty();
                revisions.retain(|id| !id.as_str().is_some_and(|id| affected.contains(id)));
                !had_revisions || !revisions.is_empty()
            });
        }
        doc
    };
    if project(before) != project(after) {
        change.error(f, "CHG-02", "unrelated rebuild data changed".into());
    }
}

fn registry_ids(change: &Change<'_>, before: &Snapshot, after: &Snapshot, f: &mut Findings) {
    let mut reserved = BTreeMap::new();
    for (path, record) in &before.documents {
        if !path.starts_with("records/")
            || !matches!(
                record["status"].as_str(),
                Some("merged" | "split" | "withdrawn")
            )
        {
            continue;
        }
        if !after.files.contains_key(path) && !after.documents.contains_key(path) {
            f.error(
                CH,
                "REG-01",
                crate::registry::REGISTRY,
                format!("retired Record file must be retained: {path}"),
                change.at,
                "",
            );
        }
        for id in std::iter::once(&record["id"])
            .chain(list(&record["aliases"]).iter().map(|alias| &alias["id"]))
        {
            if let Some(id) = id.as_str() {
                reserved.insert(id.to_ascii_lowercase(), path);
            }
        }
    }
    for (path, record) in &after.documents {
        if !path.starts_with("records/") {
            continue;
        }
        for id in std::iter::once(&record["id"])
            .chain(list(&record["aliases"]).iter().map(|alias| &alias["id"]))
        {
            if let Some(owner) = id
                .as_str()
                .and_then(|id| reserved.get(&id.to_ascii_lowercase()))
            {
                if *owner != path
                    || !matches!(
                        record["status"].as_str(),
                        Some("merged" | "split" | "withdrawn")
                    )
                {
                    f.error(
                        CH,
                        "RID-05",
                        crate::registry::REGISTRY,
                        format!("retired registry identifier reused in {path}"),
                        change.at,
                        "",
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn inspect(kind: &str, before: &Snapshot, after: &Snapshot) -> Findings {
        let mut f = Findings::default();
        let change = Change {
            commit: "c",
            at: "c",
            kind,
            declared: &["b7k2m9qd".into()],
            reviewed: false,
        };
        check(&change, before, after, &mut f);
        f
    }
    fn snapshot(history: Value) -> Snapshot {
        Snapshot {
            history: Some(history),
            ..Snapshot::default()
        }
    }
    #[test]
    fn dependent_mappings_and_revision_fix_are_scoped() {
        let h = json!({"revisions":[{"id":"b7k2m9qd","status":"active","basis":[{"relation":"rel","items":["item"]}]}],"relations":[{"id":"rel","status":"active","items":[{"id":"item"}]}],"termination":[{"relation":"rel"}]});
        let before = snapshot(h.clone());
        let mut after = snapshot(h);
        let h = after.history.as_mut().unwrap();
        h["revisions"][0]["status"] = json!("retracted");
        h["relations"][0]["status"] = json!("retracted");
        h["termination"] = json!([]);
        assert!(!inspect("revisionFix", &before, &after).failed(CH));
        after.history.as_mut().unwrap()["revisions"][0]["id"] = json!("c7k2m9qd");
        assert!(inspect("revisionFix", &before, &after).failed(CH));
        let mut before = Snapshot::default();
        let mut after = Snapshot::default();
        before.documents.insert(
            "rebuild.yaml".into(),
            json!({"mappings":[{"commit":"c","revisions":["b7k2m9qd"],"verification":"old"}]}),
        );
        after
            .documents
            .insert("rebuild.yaml".into(), json!({"mappings":[]}));
        let change = Change {
            commit: "c",
            at: "c",
            kind: "restoration",
            declared: &[],
            reviewed: false,
        };
        let mut f = Findings::default();
        mappings(
            &change,
            &before,
            &after,
            &BTreeSet::from(["b7k2m9qd".into()]),
            &mut f,
        );
        assert!(!f.failed(CH));
        before.documents.get_mut("rebuild.yaml").unwrap()["mappings"][0]["revisions"] =
            json!(["b7k2m9qd", "c7k2m9qd"]);
        mappings(
            &change,
            &before,
            &after,
            &BTreeSet::from(["b7k2m9qd".into()]),
            &mut f,
        );
        assert!(f.failed(CH));
    }

    #[test]
    fn official_correction_requires_its_source_kind_and_retired_records_survive() {
        let before = snapshot(
            json!({"revisions":[{"id":"b7k2m9qd","text":{"sources":[{"source":"s"}]}}],"sources":[{"id":"s","kind":"other"}]}),
        );
        let after = snapshot(before.history.clone().unwrap());
        assert!(inspect("officialCorrection", &before, &after).failed(CH));
        let mut after = after;
        after.history.as_mut().unwrap()["sources"][0]["kind"] = json!("officialCorrection");
        assert!(!inspect("officialCorrection", &before, &after).failed(CH));
        let mut before = Snapshot::default();
        before.documents.insert(
            "records/retired.yaml".into(),
            json!({"id":"retired","status":"merged"}),
        );
        let after = Snapshot::default();
        let f = inspect("metadata", &before, &after);
        assert!(f
            .items
            .iter()
            .any(|(_, d)| d.rule == "REG-01" && d.schema_version == crate::registry::REGISTRY));
        let empty = Snapshot::default();
        assert!(inspect("correction", &empty, &snapshot(json!({"revisions":[]}))).failed(CH));
    }

    #[test]
    fn ambiguous_ids_are_incomplete_and_unknown_kinds_cannot_change_data() {
        let change = Change {
            commit: "c",
            at: "c",
            kind: "metadata",
            declared: &[],
            reviewed: false,
        };
        for entries in [
            json!([{"id":"same"},{"id":"same"}]),
            json!([{"id":false}]),
            json!([{}]),
            json!("not an array"),
        ] {
            let mut f = Findings::default();
            assert!(!processable(
                &change,
                &snapshot(json!({"revisions":entries})),
                &mut f
            ));
            assert_eq!(f.result(CH), crate::common::ResultValue::Incomplete);
        }
        let before = Snapshot::default();
        let mut after = Snapshot::default();
        after.files.insert("history.yaml".into(), [1; 32]);
        assert!(inspect("future-kind", &before, &after).failed(CH));
        after.files.clear();
        after.files.insert("records/new.yaml".into(), [1; 32]);
        assert!(inspect("revisionAdd", &before, &after).failed(CH));
    }

    #[test]
    fn corrections_keep_unrelated_revision_metadata_and_allow_only_dependent_fields() {
        let h = json!({"revisions":[{"id":"b7k2m9qd","effective":"unchanged","text":{"sources":[{"source":"s"}],"glyphCheck":"pending"}}, {"id":"c7k2m9qd","text":{}}]});
        let before = snapshot(h.clone());
        let mut after = snapshot(h);
        after.history.as_mut().unwrap()["revisions"][0]["text"]["glyphCheck"] = json!("verified");
        assert!(!inspect("correction", &before, &after).failed(CH));
        after.history.as_mut().unwrap()["revisions"][1]["label"] = json!("unrelated");
        assert!(inspect("correction", &before, &after).failed(CH));
    }
    #[test]
    fn metadata_cannot_touch_package_bytes_and_revert_keeps_revision_records() {
        let mut before = snapshot(json!({"revisions":[{"id":"b7k2m9qd"}]}));
        before
            .files
            .insert("versions/b7k2m9qd/main.md".into(), [0; 32]);
        let mut after = snapshot(before.history.clone().unwrap());
        after
            .files
            .insert("versions/b7k2m9qd/main.md".into(), [1; 32]);
        assert!(inspect("metadata", &before, &after).failed(CH));
        after.history = Some(json!({"revisions":[]}));
        assert!(inspect("revisionFix", &before, &after).failed(CH));
    }
    #[test]
    fn untrusted_scalar_fields_do_not_panic() {
        for value in [json!(false), json!(1), json!("x"), json!(null)] {
            let before = snapshot(json!({"revisions":[{"id":"b7k2m9qd","text":value}]}));
            let after = snapshot(json!({"revisions":[{"id":"b7k2m9qd","text":{}}]}));
            assert!(inspect("correction", &before, &after).failed(CH));
        }
    }
}
