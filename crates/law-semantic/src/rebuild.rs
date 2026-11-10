//! Rebuild report checks (loj-rebuild/v1alpha1: REBR), reported apart from
//! the conformance levels (REBR-01).
use crate::common::findings::{common_type_rule, Findings};
use crate::common::{load_yaml, Check, Class, Loaded, COMMON};
use crate::repository::history::list;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

pub const REBUILD: &str = "loj-rebuild/v1alpha1";
pub const SCHEMA: &str =
    include_str!("../../../vendor/loj-spec/schemas/loj-rebuild/v1alpha1/rebuild.schema.json");
const RB: Check = Check::REBUILD;
const FILE: &str = "rebuild.yaml";

fn validator() -> &'static jsonschema::Validator {
    static V: std::sync::OnceLock<jsonschema::Validator> = std::sync::OnceLock::new();
    V.get_or_init(|| crate::common::validator(SCHEMA, &[crate::repository::history::TYPES]))
}

fn classify(schema_path: &str, instance: &str) -> (Check, &'static str, &'static str) {
    let field = instance.rsplit('/').next().unwrap_or("");
    if matches!(field, "commit" | "frozenAt") {
        return (RB, "REBR-02", REBUILD); // full hashes only
    }
    if let Some((_, rule)) = common_type_rule(schema_path) {
        if !schema_path.contains("/$defs/revisionId") && !schema_path.contains("/$defs/localId") {
            return (RB, rule, COMMON);
        }
    }
    let rule = if schema_path.starts_with("/properties/sourceChecks") {
        "REBR-03"
    } else if schema_path.starts_with("/properties/mappings")
        || schema_path.starts_with("/properties/pending")
    {
        "REBR-04"
    } else if schema_path.starts_with("/properties/differences") {
        "REBR-05"
    } else if schema_path.starts_with("/properties/record")
        || schema_path.starts_with("/properties/legacy")
    {
        "REBR-02"
    } else {
        "REBR-01"
    };
    (RB, rule, REBUILD)
}

/// Check `rebuild.yaml` against the history manifest and the version ASTs.
pub(crate) fn check(
    repo: &Path,
    history: Option<&Value>,
    asts: &HashMap<String, Option<Value>>,
    f: &mut Findings,
) {
    let Some(doc) = load(repo, f) else {
        return;
    };
    let Some(h) = history else {
        // Sources, revisions and nodes resolve against the history manifest.
        f.incomplete.insert(RB);
        return;
    };
    let glyphs_of = sources(&doc, h, f);
    // Sorted by id, so that the diagnostics come in a fixed order.
    let revisions: BTreeMap<&str, &Value> = list(&h["revisions"])
        .iter()
        .filter_map(|r| Some((r["id"].as_str()?, r)))
        .collect();
    mapped_revisions(&doc, &revisions, f);
    differences(&doc, &revisions, asts, f);
    verified_glyphs(&revisions, &glyphs_of, f);
}

/// REBR-01: read `rebuild.yaml`. `None` when there is none (the check is then
/// not applicable: a Record not moved from a legacy repository) or when it
/// cannot be used.
fn load(repo: &Path, f: &mut Findings) -> Option<Value> {
    let path = repo.join(FILE);
    if !path.exists() {
        return None;
    }
    f.applicable.insert(RB);
    let doc = match load_yaml(&path) {
        Loaded::Ok(v, _) => v,
        Loaded::Invalid(e) => {
            f.error(RB, "REBR-01", REBUILD, e, FILE, "");
            return None;
        }
        Loaded::Unreadable(e) => {
            f.incomplete.insert(RB);
            f.error(RB, "REBR-01", REBUILD, e, FILE, "");
            return None;
        }
    };
    if doc["schemaVersion"] != REBUILD {
        f.push(
            RB,
            crate::common::Diagnostic::error("REBR-01", REBUILD, "unsupported schemaVersion")
                .with_code("loj-rebuild/v1alpha1/unsupported-version")
                .at_pointer(FILE, "/schemaVersion"),
        );
        f.incomplete.insert(RB);
        return None;
    }
    f.schema(validator(), &doc, FILE, classify);
    Some(doc)
}

/// REBR-02: the Record and the checked sources are those of `history.yaml`.
/// TYPE-03: Evidence points to a Source of `history.yaml`.
/// Returns how each checked source treats glyphs (`sourceChecks[].glyphs`).
fn sources<'a>(doc: &'a Value, h: &Value, f: &mut Findings) -> HashMap<&'a str, &'a str> {
    if doc["record"]["id"] != h["record"]["id"] {
        f.error(
            RB,
            "REBR-02",
            REBUILD,
            format!(
                "record {} is not the Record {} of history.yaml",
                doc["record"]["id"], h["record"]["id"]
            ),
            FILE,
            "/record",
        );
    }
    let sources: HashSet<&str> = list(&h["sources"])
        .iter()
        .filter_map(|s| s["id"].as_str())
        .collect();
    let mut glyphs_of = HashMap::new();
    for (i, s) in list(&doc["sourceChecks"]).iter().enumerate() {
        if let Some(id) = s["source"].as_str() {
            if !sources.contains(id) {
                f.error(
                    RB,
                    "REBR-02",
                    REBUILD,
                    format!("source {id} is not a Source of history.yaml"),
                    FILE,
                    &format!("/sourceChecks/{i}/source"),
                );
            }
            glyphs_of.insert(id, s["glyphs"].as_str().unwrap_or(""));
        }
    }
    let mut refs = Vec::new();
    crate::repository::history::evidence_refs(doc, String::new(), &mut refs);
    for (p, s) in refs {
        if !sources.contains(s) {
            f.error(
                RB,
                "TYPE-03",
                COMMON,
                format!("Evidence source {s} is not a Source of history.yaml"),
                FILE,
                &p,
            );
        }
    }
    glyphs_of
}

/// REBR-04: the revisions of every mapping are in `history.yaml`.
fn mapped_revisions(doc: &Value, revisions: &BTreeMap<&str, &Value>, f: &mut Findings) {
    for (i, m) in list(&doc["mappings"]).iter().enumerate() {
        for (j, r) in list(&m["revisions"]).iter().enumerate() {
            if let Some(r) = r.as_str() {
                if !revisions.contains_key(r) {
                    f.error(
                        RB,
                        "REBR-04",
                        REBUILD,
                        format!("revision {r} is not in history.yaml"),
                        FILE,
                        &format!("/mappings/{i}/revisions/{j}"),
                    );
                }
            }
        }
    }
}

/// REBR-05: a difference names a revision of `history.yaml` and, when it
/// names a node, a node of that revision.
fn differences(
    doc: &Value,
    revisions: &BTreeMap<&str, &Value>,
    asts: &HashMap<String, Option<Value>>,
    f: &mut Findings,
) {
    for (i, d) in list(&doc["differences"]).iter().enumerate() {
        let p = format!("/differences/{i}");
        let Some(r) = d["revision"].as_str() else {
            continue;
        };
        if !revisions.contains_key(r) {
            f.error(
                RB,
                "REBR-05",
                REBUILD,
                format!("revision {r} is not in history.yaml"),
                FILE,
                &format!("{p}/revision"),
            );
            continue;
        }
        let Some(node) = d["node"].as_str() else {
            continue;
        };
        match asts.get(r) {
            Some(Some(ast)) => {
                if !crate::repository::glyphs::Texts::from_ast(ast).contains(node) {
                    f.error(
                        RB,
                        "REBR-05",
                        REBUILD,
                        format!("node {node} is not in revision {r}"),
                        FILE,
                        &format!("{p}/node"),
                    );
                }
            }
            _ => f.note(
                RB,
                Class::Undetermined,
                "REBR-05",
                REBUILD,
                format!("node {node} of revision {r} not checked: no valid AST"),
                FILE,
                &format!("{p}/node"),
            ),
        }
    }
}

/// REBR-03 / GLYPH-10: verified glyphs need a source that preserves glyphs.
fn verified_glyphs(
    revisions: &BTreeMap<&str, &Value>,
    glyphs_of: &HashMap<&str, &str>,
    f: &mut Findings,
) {
    for (id, r) in revisions {
        if r["text"]["glyphs"] != "verified" {
            continue;
        }
        let preserved = list(&r["text"]["sources"]).iter().any(|e| {
            e["source"]
                .as_str()
                .and_then(|s| glyphs_of.get(s))
                .is_some_and(|g| *g == "preserved")
        });
        if !preserved {
            f.error(
                RB,
                "REBR-03",
                REBUILD,
                format!("revision {id} has text.glyphs verified without a preserved source"),
                FILE,
                "/sourceChecks",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn found(f: &Findings) -> Vec<(&str, &str)> {
        f.items
            .iter()
            .map(|(_, d)| (d.rule.as_str(), d.message.as_str()))
            .collect()
    }

    #[test]
    fn sources_and_the_record_are_those_of_the_history() {
        let h = json!({"record": {"id": "r1"}, "sources": [{"id": "s1"}, {"id": "s2"}]});
        let doc = json!({
            "record": {"id": "r2"},
            "sourceChecks": [
                {"source": "s1", "glyphs": "preserved"},
                {"source": "s9", "glyphs": "normalized"},
                {"glyphs": "preserved"},
            ],
            "mappings": [{"evidence": [{"source": "s2"}, {"source": "s8"}]}],
        });
        let mut f = Findings::default();
        let glyphs_of = sources(&doc, &h, &mut f);
        assert_eq!(glyphs_of.len(), 2);
        assert_eq!(glyphs_of["s1"], "preserved");
        assert_eq!(glyphs_of["s9"], "normalized");
        assert_eq!(
            found(&f),
            [
                (
                    "REBR-02",
                    "record \"r2\" is not the Record \"r1\" of history.yaml"
                ),
                ("REBR-02", "source s9 is not a Source of history.yaml"),
                (
                    "TYPE-03",
                    "Evidence source s8 is not a Source of history.yaml"
                ),
            ]
        );
    }

    #[test]
    fn verified_glyphs_are_reported_in_the_order_of_the_revision_ids() {
        let h = json!({"revisions": [
            {"id": "c", "text": {"glyphs": "verified", "sources": [{"source": "s2"}]}},
            {"id": "a", "text": {"glyphs": "verified", "sources": []}},
            {"id": "b", "text": {"glyphs": "verified", "sources": [{"source": "s1"}]}},
            {"id": "d", "text": {"glyphs": "unverified"}},
            {"text": {"glyphs": "verified"}},
        ]});
        let revisions: BTreeMap<&str, &Value> = list(&h["revisions"])
            .iter()
            .filter_map(|r| Some((r["id"].as_str()?, r)))
            .collect();
        let glyphs_of = HashMap::from([("s1", "preserved"), ("s2", "normalized")]);
        let mut f = Findings::default();
        verified_glyphs(&revisions, &glyphs_of, &mut f);
        assert_eq!(
            found(&f),
            [
                (
                    "REBR-03",
                    "revision a has text.glyphs verified without a preserved source"
                ),
                (
                    "REBR-03",
                    "revision c has text.glyphs verified without a preserved source"
                ),
            ]
        );
    }

    #[test]
    fn mappings_and_differences_name_revisions_and_nodes_that_exist() {
        let h = json!({"revisions": [{"id": "v1"}, {"id": "v2"}]});
        let revisions: BTreeMap<&str, &Value> = list(&h["revisions"])
            .iter()
            .filter_map(|r| Some((r["id"].as_str()?, r)))
            .collect();
        let doc = json!({
            "mappings": [{"revisions": ["v1", "v3"]}],
            "differences": [
                {"revision": "v1", "node": "law/a"},
                {"revision": "v1", "node": "law/b"},
                {"revision": "v2", "node": "law/a"},
                {"revision": "v4", "node": "law/a"},
                {"revision": "v1"},
            ],
        });
        let ast = json!({"root": {"kind": "Law", "id": "law",
            "content": [{"kind": "Article", "id": "law/a", "content": []}]}});
        let asts = HashMap::from([("v1".to_owned(), Some(ast)), ("v2".to_owned(), None)]);
        let mut f = Findings::default();
        mapped_revisions(&doc, &revisions, &mut f);
        differences(&doc, &revisions, &asts, &mut f);
        assert_eq!(
            found(&f),
            [
                ("REBR-04", "revision v3 is not in history.yaml"),
                ("REBR-05", "node law/b is not in revision v1"),
                (
                    "REBR-05",
                    "node law/a of revision v2 not checked: no valid AST"
                ),
                ("REBR-05", "revision v4 is not in history.yaml"),
            ]
        );
        assert_eq!(f.items[2].1.class, Class::Undetermined);
    }
}
