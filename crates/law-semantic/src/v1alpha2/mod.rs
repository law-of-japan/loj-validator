//! `loj/v1alpha2` version packages: L0 and L1 (spec/v1alpha2).
//!
//! Built on the v1alpha1 pipeline (manifest, CommonMark regions, markers,
//! rank stack, IDs, byte positions) with unnumbered paragraphs (PARA-01),
//! tables (TABLE-01..03), the preamble and enact statement (PRE-01..02),
//! the 條/ノ heading forms and the spaced supplement heading.
mod directives;
mod kind;
mod manifest;
mod syntax;

use crate::common::{validator, Check, Diagnostic, Report, ResultValue};
use serde_json::{json, Map, Value};
use std::path::Path;

pub const SCHEMA_VERSION: &str = "loj/v1alpha2";
pub const MANIFEST_SCHEMA: &str =
    include_str!("../../../../vendor/loj-spec/schemas/v1alpha2/manifest.schema.json");
pub const AST_SCHEMA: &str =
    include_str!("../../../../vendor/loj-spec/schemas/v1alpha2/ast.schema.json");
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 128 * 1024 * 1024;

/// One input file as it was read (SRC-01).
pub(super) struct Source {
    path: String,
    /// The text, or the raw bytes when they are not UTF-8.
    content: Result<String, Vec<u8>>,
}

impl Source {
    fn to_json(&self) -> Value {
        match &self.content {
            Ok(text) => json!({"path":self.path,"text":text}),
            Err(bytes) => json!({"path":self.path,"bytes":bytes}),
        }
    }
}

/// Diagnostics are tagged with the level of the stage that found them
/// (spec/v1alpha2/10-validation.md: manifest reading is L0, structure is L1).
pub(super) struct Context {
    diagnostics: Vec<(Check, Diagnostic)>,
    sources: Vec<Source>,
    /// Output records of [`PackageCheck::positions`].
    positions: Vec<Value>,
    source_map: Map<String, Value>,
    level: Check,
    /// L0 could not be completed (IO, limits, unsupported version).
    incomplete: bool,
    /// Some listed document could not be read; L1 cannot be completed.
    missing: bool,
}

impl Default for Context {
    fn default() -> Self {
        Context {
            diagnostics: Vec::new(),
            sources: Vec::new(),
            positions: Vec::new(),
            source_map: Map::new(),
            level: Check::L0,
            incomplete: false,
            missing: false,
        }
    }
}

impl Context {
    fn diag(&mut self, rule: &str, message: &str, path: &str, start: usize, end: usize) {
        self.diagnostics.push((
            self.level,
            Diagnostic::error(rule, SCHEMA_VERSION, message).at(path, Some((start, end))),
        ));
    }
    /// A diagnostic with an implementation code of its own instead of the
    /// one derived from the rule.
    fn diag_with_code(
        &mut self,
        rule: &str,
        code: &str,
        message: &str,
        path: &str,
        span: std::ops::Range<usize>,
    ) {
        self.diagnostics.push((
            self.level,
            Diagnostic::error(rule, SCHEMA_VERSION, message)
                .with_code(code)
                .at(path, Some((span.start, span.end))),
        ));
    }
    fn position(&mut self, id: &str, path: &str, start: usize, end: usize) {
        // Every entry of the source map is an array: nothing else is inserted.
        if let Some(spans) = self
            .source_map
            .entry(id.to_owned())
            .or_insert_with(|| json!([]))
            .as_array_mut()
        {
            spans.push(json!({"path":path,"start":start,"end":end}));
        } else {
            debug_assert!(false, "a source map entry that is not an array: {id}");
        }
        self.positions
            .push(json!({"nodeId":id,"path":path,"start":start,"end":end}));
    }
}

/// Result of checking one version package.
#[derive(Debug, Clone)]
pub struct PackageCheck {
    /// Report in the shape of `report.schema.json`.
    pub report: Report,
    /// Complete semantic AST; only when L1 passed (AST-02).
    pub ast: Option<Value>,
    /// Node positions `{nodeId, path, start, end}` and text fragments.
    pub positions: Vec<Value>,
    /// Positions per node ID (SRC-01).
    pub source_map: Map<String, Value>,
    /// Original inputs `{path, text}` (SRC-01).
    pub sources: Vec<Value>,
}

/// Validate a `law.yaml` path or its directory at L0 and/or L1.
pub fn check(input: &Path, levels: &[Check]) -> PackageCheck {
    let mut ctx = Context::default();
    let path = if input.is_dir() {
        input.join("law.yaml")
    } else {
        input.to_owned()
    };
    let loaded = manifest::load(&path, &mut ctx);
    let mut ast = None;
    let manifest_ok = loaded.is_some();
    if let Some((manifest, documents)) = loaded {
        for (_, source, _) in &documents {
            // L0: each file is parsed as CommonMark (never fails, MD-03).
            for _ in pulldown_cmark::Parser::new(source) {}
        }
        if levels.contains(&Check::L1) && !ctx.missing && !ctx.incomplete {
            ctx.level = Check::L1;
            if manifest["unsupportedFeatures"]
                .as_array()
                .is_some_and(|a| !a.is_empty())
            {
                let end = ctx
                    .sources
                    .first()
                    .and_then(|s| s.content.as_ref().ok())
                    .map_or(0, String::len);
                ctx.diag(
                    "BASIC-03",
                    "Declared unsupported legal structure",
                    "law.yaml",
                    0,
                    end,
                );
            }
            let tree = syntax::build(&manifest, &documents, &mut ctx);
            if !ast_validator().is_valid(&tree) {
                ctx.diag(
                    "AST-01",
                    "Constructed tree fails AST schema",
                    "law.yaml",
                    0,
                    0,
                );
            }
            ast = Some(tree);
        }
    }
    let failed = |level: Check| {
        ctx.diagnostics
            .iter()
            .any(|(l, d)| *l == level && d.is_error())
    };
    let mut report = Report::default();
    report
        .target
        .insert("path".into(), json!(path.to_string_lossy()));
    for &level in levels {
        let result = if level == Check::L0 {
            if ctx.incomplete {
                ResultValue::Incomplete
            } else if failed(Check::L0) {
                ResultValue::Fail
            } else {
                ResultValue::Pass
            }
        } else if level == Check::L1 {
            if !manifest_ok || ctx.missing || ctx.incomplete {
                ResultValue::Incomplete
            } else if failed(Check::L1) {
                ResultValue::Fail
            } else {
                ResultValue::Pass
            }
        } else {
            continue;
        };
        report.set(level, result);
    }
    let l1_pass = report.result(Check::L1) == Some(ResultValue::Pass);
    report.diagnostics = ctx.diagnostics.into_iter().map(|(_, d)| d).collect();
    PackageCheck {
        report,
        ast: if l1_pass { ast } else { None },
        positions: ctx.positions,
        source_map: ctx.source_map,
        sources: ctx.sources.iter().map(Source::to_json).collect(),
    }
}

fn ast_validator() -> &'static jsonschema::Validator {
    static V: std::sync::OnceLock<jsonschema::Validator> = std::sync::OnceLock::new();
    V.get_or_init(|| validator(AST_SCHEMA, &[]))
}
fn manifest_validator() -> &'static jsonschema::Validator {
    static V: std::sync::OnceLock<jsonschema::Validator> = std::sync::OnceLock::new();
    V.get_or_init(|| validator(MANIFEST_SCHEMA, &[]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_are_returned_as_text_or_as_the_bytes_that_were_read() {
        let text = Source {
            path: "main.md".into(),
            content: Ok("第一条\r\n".into()),
        };
        assert_eq!(
            text.to_json(),
            json!({"path": "main.md", "text": "第一条\r\n"})
        );
        // Not UTF-8: the bytes are kept as they are, not replaced or dropped.
        let bytes = Source {
            path: "main.md".into(),
            content: Err(vec![0xff, b'a', 0x80]),
        };
        assert_eq!(
            bytes.to_json(),
            json!({"path": "main.md", "bytes": [255, 97, 128]})
        );
    }

    #[test]
    fn a_file_that_is_not_utf8_is_reported_at_its_start_and_kept_as_bytes() {
        let dir = std::env::temp_dir().join(format!("loj2-source-bytes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("law.yaml"), b"schemaVersion: \xff\n").unwrap();
        let checked = check(&dir, &[Check::L0]);
        std::fs::remove_dir_all(&dir).unwrap();
        let d = &checked.report.diagnostics[0];
        assert_eq!(d.rule, "BASIC-02");
        assert_eq!(
            d.location,
            Some(json!({"path": "law.yaml", "start": 0, "end": 0}))
        );
        assert_eq!(checked.sources.len(), 1);
        assert_eq!(checked.sources[0]["path"], "law.yaml");
        assert_eq!(
            checked.sources[0]["bytes"],
            json!(b"schemaVersion: \xff\n".to_vec())
        );
        assert!(checked.ast.is_none());
    }

    /// The builder reads manifest fields with `expect`; the loader returns
    /// before it for any manifest that its schema refuses.
    #[test]
    fn a_manifest_that_fails_its_schema_is_reported_and_never_built() {
        let law = r#""law": {"id": "20990101-act-1", "title": "架空試験法", "type": "act",
            "number": "架空法律第一号", "promulgatedAt": "2099-01-01", "language": "ja"}"#;
        let cases = [
            // No title, no id, a title that is not a string.
            r#"{"schemaVersion": "loj/v1alpha2", "law": {"id": "20990101-act-1"},
                "documents": [{"path": "main.md", "parent": "main"}]}"#
                .to_owned(),
            r#"{"schemaVersion": "loj/v1alpha2", "law": {"title": "架空試験法"},
                "documents": [{"path": "main.md", "parent": "main"}]}"#
                .to_owned(),
            // No documents, documents that are not an array, entries without fields.
            format!(r#"{{"schemaVersion": "loj/v1alpha2", {law}}}"#),
            format!(r#"{{"schemaVersion": "loj/v1alpha2", {law}, "documents": "main.md"}}"#),
            format!(
                r#"{{"schemaVersion": "loj/v1alpha2", {law}, "documents": [{{"parent": "main"}}]}}"#
            ),
            format!(
                r#"{{"schemaVersion": "loj/v1alpha2", {law}, "documents": [{{"path": "main.md"}}]}}"#
            ),
            format!(
                r#"{{"schemaVersion": "loj/v1alpha2", {law}, "documents": [{{"path": 1, "parent": 2}}]}}"#
            ),
            // A supplement without a key, an appendix without a title.
            format!(
                r#"{{"schemaVersion": "loj/v1alpha2", {law},
                "documents": [{{"path": "main.md", "parent": "main"}}],
                "supplements": [{{"title": "附則"}}]}}"#
            ),
            format!(
                r#"{{"schemaVersion": "loj/v1alpha2", {law},
                "documents": [{{"path": "main.md", "parent": "main"}}],
                "appendices": [{{"key": "table-1"}}]}}"#
            ),
        ];
        for (i, manifest) in cases.iter().enumerate() {
            let dir = std::env::temp_dir()
                .join(format!("loj2-schema-refused-{}-{i}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("law.yaml"), manifest).unwrap();
            std::fs::write(dir.join("main.md"), "# 第一条\n").unwrap();
            let checked = check(&dir, &[Check::L0, Check::L1]);
            std::fs::remove_dir_all(&dir).unwrap();
            assert!(checked.ast.is_none(), "{manifest}");
            assert_eq!(
                checked.report.result(Check::L0),
                Some(ResultValue::Fail),
                "{manifest}"
            );
            assert!(
                checked
                    .report
                    .diagnostics
                    .iter()
                    .all(|d| d.rule == "MAN-01"),
                "{manifest}"
            );
        }
    }

    #[test]
    fn an_unsupported_version_has_its_own_code() {
        let dir = std::env::temp_dir().join(format!("loj2-version-code-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("law.yaml"), r#"{"schemaVersion": "loj/v9"}"#).unwrap();
        let checked = check(&dir, &[Check::L0]);
        std::fs::remove_dir_all(&dir).unwrap();
        let d = &checked.report.diagnostics[0];
        assert_eq!(
            (d.rule.as_str(), d.code.as_str()),
            ("BASIC-01", "loj/v1alpha2/unsupported-version")
        );
        assert_eq!(
            d.location,
            Some(json!({"path": "law.yaml", "start": 0, "end": 27}))
        );
        assert_eq!(
            checked.report.result(Check::L0),
            Some(ResultValue::Incomplete)
        );
    }
}
