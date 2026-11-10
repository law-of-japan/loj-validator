//! L3 evidence of `glyphs.yaml` (loj-glyphs/v1alpha1).
use super::history::list;
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
    let file = notes.unwrap_or("");
    let doc = notes.and_then(|n| load(repo, n, f));
    let entries = doc.as_ref().map_or(&[][..], |d| list(&d["notes"]));
    if let Some(d) = &doc {
        document(d, entries, revision, file, sources, f);
    }
    // GLYPH-11 / VAL-02: notes address L1 node IDs; without a valid AST they are not judged.
    let Some(ast) = ast else {
        f.incomplete.insert(EV);
        return;
    };
    let texts = Texts::from_ast(ast);
    let covered = located(entries, &texts, file, f);
    unnoted(&texts, &covered, file, f);
}

/// GLYPH-01: read the notes file `n`. `None` when it cannot be used.
fn load(repo: &Path, n: &str, f: &mut Findings) -> Option<Value> {
    match load_yaml(&repo.join(n)) {
        Loaded::Unreadable(_) => {
            // HIST-08 reports the missing file; the evidence cannot be judged.
            f.incomplete.insert(EV);
            None
        }
        Loaded::Invalid(e) => {
            f.error(EV, "GLYPH-01", GLYPHS, e, n, "");
            None
        }
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
                None
            } else {
                f.schema(validator(), &v, n, classify);
                Some(v)
            }
        }
    }
}

/// GLYPH-01 (the revision), GLYPH-05 (note ids, candidates) and TYPE-03
/// (Evidence sources): what the notes file says without the AST.
fn document(
    d: &Value,
    list: &[Value],
    revision: &str,
    file: &str,
    sources: &HashSet<String>,
    f: &mut Findings,
) {
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

/// GLYPH-04: where `recorded` is in `text`, if it is at code point `offset`.
/// The offset comes from the input: it may exceed any text, or `usize`.
pub(crate) fn recorded_at(
    text: &[char],
    offset: u64,
    recorded: &[char],
) -> Option<std::ops::Range<usize>> {
    let start = usize::try_from(offset).ok()?;
    let range = start..start.checked_add(recorded.len())?;
    (text.get(range.clone()) == Some(recorded)).then_some(range)
}

/// Code point ranges that notes cover, per node ID and field (`true` is the
/// display field).
type Covered = HashMap<(String, bool), Vec<(usize, usize)>>;

/// GLYPH-04: each note names a node of this version and the recorded text is
/// at its offset. Returns what the notes cover.
fn located(list: &[Value], texts: &Texts, file: &str, f: &mut Findings) -> Covered {
    let mut covered = Covered::new();
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
        let Some(range) = recorded_at(&target, offset, &recorded) else {
            f.error(
                EV,
                "GLYPH-04",
                GLYPHS,
                format!("recorded text is not at offset {offset} of {node}"),
                file,
                &format!("{p}/offset"),
            );
            continue;
        };
        // An empty recorded text still marks the code point at its offset.
        let end = range.end.max(range.start.saturating_add(1));
        covered
            .entry((node.to_owned(), display))
            .or_default()
            .push((range.start, end));
    }
    covered
}

/// GLYPH-02 / GLYPH-11: every `〓`, compatibility ideograph and variation
/// sequence of the text has a note. GLYPH-03: no forbidden code point.
fn unnoted(texts: &Texts, covered: &Covered, file: &str, f: &mut Findings) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn texts() -> Texts {
        Texts::from_ast(&json!({"root": {
            "kind": "Law", "id": "law", "display": "試験法",
            "content": [{"kind": "Paragraph", "id": "law/p", "display": "",
                "content": [{"text": "〓と亞\u{fa11}の字"}]}],
        }}))
    }

    fn found(f: &Findings) -> Vec<(&str, String)> {
        f.items
            .iter()
            .map(|(_, d)| {
                let pointer = d.location.as_ref().unwrap()["pointer"].as_str().unwrap();
                (d.rule.as_str(), pointer.to_owned())
            })
            .collect()
    }

    #[test]
    fn notes_are_located_by_code_point_offset() {
        let notes = json!([
            {"node": "law/p", "offset": 0, "recorded": "〓"},
            {"node": "law/p", "offset": 3, "recorded": "\u{fa11}"},
            {"node": "law", "field": "display", "offset": 0, "recorded": "試験"},
            // Wrong text, past the end, and an unknown node.
            {"node": "law/p", "offset": 1, "recorded": "亞"},
            {"node": "law/p", "offset": 6, "recorded": "字"},
            {"node": "law/q", "offset": 0, "recorded": "字"},
            // Incomplete notes are left to the schema check.
            {"node": "law/p", "recorded": "字"},
        ]);
        let mut f = Findings::default();
        let covered = located(list(&notes), &texts(), "glyphs.yaml", &mut f);
        assert_eq!(covered[&("law/p".to_owned(), false)], [(0, 1), (3, 4)]);
        assert_eq!(covered[&("law".to_owned(), true)], [(0, 2)]);
        assert_eq!(
            found(&f),
            [
                ("GLYPH-04", "/notes/3/offset".to_owned()),
                ("GLYPH-04", "/notes/4/offset".to_owned()),
                ("GLYPH-04", "/notes/5/node".to_owned()),
            ]
        );
    }

    #[test]
    fn an_offset_beyond_any_text_is_a_finding_not_a_panic() {
        for offset in [u64::MAX, u64::MAX - 1, u64::from(u32::MAX) + 1, 7] {
            let notes = json!([{"node": "law/p", "offset": offset, "recorded": "字の"}]);
            let mut f = Findings::default();
            let covered = located(list(&notes), &texts(), "glyphs.yaml", &mut f);
            assert!(covered.is_empty(), "{offset}");
            assert_eq!(found(&f), [("GLYPH-04", "/notes/0/offset".to_owned())]);
            assert_eq!(
                f.items[0].1.message,
                format!("recorded text is not at offset {offset} of law/p")
            );
        }
    }

    #[test]
    fn recorded_text_is_found_only_at_its_offset() {
        let text: Vec<char> = "第一条の二".chars().collect();
        let chars = |s: &str| s.chars().collect::<Vec<char>>();
        assert_eq!(recorded_at(&text, 1, &chars("一条")), Some(1..3));
        assert_eq!(recorded_at(&text, 4, &chars("二")), Some(4..5));
        assert_eq!(recorded_at(&text, 5, &[]), Some(5..5));
        assert_eq!(recorded_at(&text, 0, &chars("一条")), None);
        assert_eq!(recorded_at(&text, 4, &chars("二の")), None);
        assert_eq!(recorded_at(&text, 6, &[]), None);
        for offset in [u64::MAX, u64::MAX - 1, u64::from(u32::MAX) + 1] {
            assert_eq!(recorded_at(&text, offset, &chars("二")), None, "{offset}");
            assert_eq!(recorded_at(&text, offset, &[]), None, "{offset}");
        }
    }

    #[test]
    fn characters_that_need_a_note_are_reported_once_each() {
        let mut f = Findings::default();
        let covered = Covered::from([(("law/p".to_owned(), false), vec![(0, 1)])]);
        unnoted(&texts(), &covered, "glyphs.yaml", &mut f);
        // `〓` at 0 is covered; the compatibility ideograph at 3 is not.
        let messages: Vec<_> = f.items.iter().map(|(_, d)| d.message.as_str()).collect();
        assert_eq!(
            messages,
            ["U+FA11 at text offset 3 of law/p has no glyph note"]
        );
        assert_eq!(f.items[0].1.rule, "GLYPH-11");
    }
}
