//! Rebuild report checks (loj-rebuild/v1alpha1: REBR), reported apart from
//! the conformance levels (REBR-01).
use crate::common::findings::{common_type_rule, Findings};
use crate::common::{load_yaml, Check, Class, Loaded, COMMON};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
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
    let path = repo.join(FILE);
    if !path.exists() {
        return; // notApplicable: a Record not moved from a legacy repository
    }
    f.applicable.insert(RB);
    let doc = match load_yaml(&path) {
        Loaded::Ok(v, _) => v,
        Loaded::Invalid(e) => return f.error(RB, "REBR-01", REBUILD, e, FILE, ""),
        Loaded::Unreadable(e) => {
            f.incomplete.insert(RB);
            return f.error(RB, "REBR-01", REBUILD, e, FILE, "");
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
        return;
    }
    f.schema(validator(), &doc, FILE, classify);
    let Some(h) = history else {
        // Sources, revisions and nodes resolve against the history manifest.
        f.incomplete.insert(RB);
        return;
    };
    let empty = Vec::new();
    let list = |v: &Value| v.as_array().unwrap_or(&empty).clone();
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
    let sources: HashSet<String> = list(&h["sources"])
        .iter()
        .filter_map(|s| s["id"].as_str().map(str::to_owned))
        .collect();
    let mut glyphs_of: HashMap<String, String> = HashMap::new();
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
            glyphs_of.insert(id.to_owned(), s["glyphs"].as_str().unwrap_or("").to_owned());
        }
    }
    let mut refs = Vec::new();
    crate::repository::history::evidence_refs(&doc, String::new(), &mut refs);
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
    let revisions: HashMap<String, Value> = list(&h["revisions"])
        .into_iter()
        .filter_map(|r| r["id"].as_str().map(|i| (i.to_owned(), r.clone())))
        .collect();
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
        if let Some(node) = d["node"].as_str() {
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
    // REBR-03 / GLYPH-10: verified glyphs need a source that preserves glyphs.
    for (id, r) in &revisions {
        if r["text"]["glyphs"] != "verified" {
            continue;
        }
        let preserved = list(&r["text"]["sources"]).iter().any(|e| {
            e["source"]
                .as_str()
                .and_then(|s| glyphs_of.get(s))
                .is_some_and(|g| g == "preserved")
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
