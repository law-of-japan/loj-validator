//! L3 evidence of `glyphs.yaml` (loj-glyphs/v1alpha1).
use super::{EVIDENCE as EV, GLYPHS};
use crate::common::findings::{common_type_rule, Findings};
use crate::common::{load_yaml, Check, Loaded, COMMON};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub const SCHEMA: &str =
    include_str!("../../../../vendor/loj-spec/schemas/loj-glyphs/v1alpha1/glyphs.schema.json");

fn validator() -> &'static jsonschema::Validator {
    static V: std::sync::OnceLock<jsonschema::Validator> = std::sync::OnceLock::new();
    V.get_or_init(|| crate::common::validator(SCHEMA, &[super::history::TYPES]))
}

fn classify(schema_path: &str, instance: &str) -> (Check, &'static str, &'static str) {
    if let Some((check, rule)) = common_type_rule(schema_path) {
        if !schema_path.contains("/$defs/revisionId") {
            return (check, rule, COMMON);
        }
    }
    let field = instance.rsplit('/').next().unwrap_or("");
    if schema_path.starts_with("/properties/notes/items/allOf/")
        || matches!(
            field,
            "candidates" | "basis" | "description" | "kind" | "id"
        )
        || instance.contains("/candidates/")
    {
        return (EV, "GLYPH-05", GLYPHS);
    }
    if matches!(field, "node" | "offset" | "recorded" | "field") {
        return (EV, "GLYPH-04", GLYPHS);
    }
    (EV, "GLYPH-01", GLYPHS)
}

/// Text addressed by notes: per node ID, own text (children excluded) and display.
pub(crate) struct Texts {
    nodes: HashMap<String, (String, String, bool)>,
}

impl Texts {
    pub(crate) fn from_ast(ast: &Value) -> Texts {
        fn walk(n: &Value, out: &mut HashMap<String, (String, String, bool)>) {
            let mut own = String::new();
            for c in n["content"].as_array().into_iter().flatten() {
                if let Some(t) = c["text"].as_str() {
                    own.push_str(t);
                } else {
                    walk(c, out);
                }
            }
            // Headings and the title carry their own characters in `display`.
            let heading = matches!(
                n["kind"].as_str().unwrap_or(""),
                "Law"
                    | "Part"
                    | "Chapter"
                    | "Section"
                    | "Subsection"
                    | "Division"
                    | "Article"
                    | "SupplementaryProvision"
                    | "AppendedMaterial"
            );
            if let Some(id) = n["id"].as_str() {
                let display = n["display"].as_str().unwrap_or("").to_owned();
                out.insert(id.to_owned(), (own, display, heading));
            }
        }
        let mut nodes = HashMap::new();
        walk(&ast["root"], &mut nodes);
        Texts { nodes }
    }
    pub(crate) fn contains(&self, id: &str) -> bool {
        self.nodes.contains_key(id)
    }
}

fn compat(c: char) -> bool {
    matches!(c as u32, 0xF900..=0xFAFF | 0x2F800..=0x2FA1F)
}
fn selector(c: char) -> bool {
    matches!(c as u32, 0xFE00..=0xFE0F | 0xE0100..=0xE01EF)
}
fn forbidden(c: char) -> bool {
    matches!(c as u32, 0xFFFD | 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD)
}

/// Check the glyph notes of one revision. `notes` is the glyphNotes path
/// (relative to the repository), `ast` the package AST when L1 passed.
pub(crate) fn check(
    repo: &Path,
    revision: &str,
    notes: Option<&str>,
    ast: Option<&Value>,
    sources: &HashSet<String>,
    f: &mut Findings,
) {
    f.applicable.insert(EV);
    let mut doc = None;
    let file = notes.unwrap_or("");
    if let Some(n) = notes {
        match load_yaml(&repo.join(n)) {
            Loaded::Unreadable(_) => {
                // HIST-08 reports the missing file; the evidence cannot be judged.
                f.incomplete.insert(EV);
            }
            Loaded::Invalid(e) => f.error(EV, "GLYPH-01", GLYPHS, e, n, ""),
            Loaded::Ok(v, _) => {
                if v["schemaVersion"] != GLYPHS {
                    f.error(
                        EV,
                        "GLYPH-01",
                        GLYPHS,
                        "unsupported schemaVersion",
                        n,
                        "/schemaVersion",
                    );
                    f.incomplete.insert(EV);
                } else {
                    f.schema(validator(), &v, n, classify);
                    doc = Some(v);
                }
            }
        }
    }
    let empty = Vec::new();
    let list = doc
        .as_ref()
        .and_then(|d| d["notes"].as_array())
        .unwrap_or(&empty);
    if let Some(d) = &doc {
        if d["revision"].as_str().is_some_and(|r| r != revision) {
            f.error(
                EV,
                "GLYPH-01",
                GLYPHS,
                format!("notes are for revision {}, not {revision}", d["revision"]),
                file,
                "/revision",
            );
        }
        let mut ids = HashSet::new();
        for (i, n) in list.iter().enumerate() {
            let p = format!("/notes/{i}");
            if let Some(id) = n["id"].as_str() {
                if !ids.insert(id) {
                    f.error(
                        EV,
                        "GLYPH-05",
                        GLYPHS,
                        format!("duplicate note id {id}"),
                        file,
                        &p,
                    );
                }
            }
            for (j, e) in n["evidence"].as_array().into_iter().flatten().enumerate() {
                if let Some(s) = e["source"].as_str() {
                    if !sources.contains(s) {
                        f.error(
                            EV,
                            "TYPE-03",
                            COMMON,
                            format!("Evidence source {s} is not a Source of the history manifest"),
                            file,
                            &format!("{p}/evidence/{j}"),
                        );
                    }
                }
            }
            if n["kind"] == "ambiguous" && n["recorded"] != "〓" {
                let has = n["candidates"]
                    .as_array()
                    .is_some_and(|c| c.contains(&n["recorded"]));
                if !has {
                    f.error(
                        EV,
                        "GLYPH-05",
                        GLYPHS,
                        "candidates must include the recorded text",
                        file,
                        &format!("{p}/candidates"),
                    );
                }
            }
        }
    }
    // GLYPH-11 / VAL-02: notes address L1 node IDs; without a valid AST they are not judged.
    let Some(ast) = ast else {
        f.incomplete.insert(EV);
        return;
    };
    let texts = Texts::from_ast(ast);
    // Covered code point ranges per (node, field).
    let mut covered: HashMap<(String, bool), Vec<(usize, usize)>> = HashMap::new();
    for (i, n) in list.iter().enumerate() {
        let p = format!("/notes/{i}");
        let (Some(node), Some(offset), Some(recorded)) = (
            n["node"].as_str(),
            n["offset"].as_u64(),
            n["recorded"].as_str(),
        ) else {
            continue;
        };
        let display = n["field"] == "display";
        let Some((own, disp, _)) = texts.nodes.get(node) else {
            f.error(
                EV,
                "GLYPH-04",
                GLYPHS,
                format!("note node {node} does not resolve in this version"),
                file,
                &format!("{p}/node"),
            );
            continue;
        };
        let target: Vec<char> = if display { disp } else { own }.chars().collect();
        let recorded: Vec<char> = recorded.chars().collect();
        let (o, len) = (offset as usize, recorded.len());
        if o + len > target.len() || target[o..o + len] != recorded[..] {
            f.error(
                EV,
                "GLYPH-04",
                GLYPHS,
                format!("recorded text is not at offset {o} of {node}"),
                file,
                &format!("{p}/offset"),
            );
            continue;
        }
        covered
            .entry((node.to_owned(), display))
            .or_default()
            .push((o, o + len.max(1)));
    }
    let mut ids: Vec<&String> = texts.nodes.keys().collect();
    ids.sort();
    for id in ids {
        let (own, disp, heading) = &texts.nodes[id];
        for (display, s) in [(false, own), (true, disp)] {
            if display && !heading {
                continue; // a container's display token is part of its own text
            }
            let chars: Vec<char> = s.chars().collect();
            let ranges = covered.get(&(id.clone(), display));
            let is_covered =
                |k: usize| ranges.is_some_and(|r| r.iter().any(|(a, b)| *a <= k && k < *b));
            let field = if display { "display" } else { "text" };
            for (k, c) in chars.iter().enumerate() {
                let rule = if *c == '〓' {
                    Some("GLYPH-02")
                } else if compat(*c) || chars.get(k + 1).is_some_and(|n| selector(*n)) {
                    Some("GLYPH-11")
                } else {
                    None
                };
                if let Some(rule) = rule {
                    if !is_covered(k) {
                        f.error(
                            EV,
                            rule,
                            GLYPHS,
                            format!(
                                "U+{:04X} at {field} offset {k} of {id} has no glyph note",
                                *c as u32
                            ),
                            file,
                            "/notes",
                        );
                    }
                }
                if forbidden(*c) {
                    f.error(
                        EV,
                        "GLYPH-03",
                        GLYPHS,
                        format!(
                            "U+{:04X} must not be placed in the text (use 〓)",
                            *c as u32
                        ),
                        file,
                        "/notes",
                    );
                }
            }
        }
    }
}
