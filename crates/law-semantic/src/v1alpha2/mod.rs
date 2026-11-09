//! `loj/v1alpha2` version packages: L0 and L1 (spec/v1alpha2).
//!
//! Built on the v1alpha1 pipeline (manifest, CommonMark regions, markers,
//! rank stack, IDs, byte positions) with unnumbered paragraphs (PARA-01),
//! tables (TABLE-01..03), the preamble and enact statement (PRE-01..02),
//! the 條/ノ heading forms and the spaced supplement heading.
mod directives;
mod manifest;
mod markdown;
mod number;
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

/// Diagnostics are tagged with the level of the stage that found them
/// (spec/v1alpha2/10-validation.md: manifest reading is L0, structure is L1).
pub(super) struct Context {
    diagnostics: Vec<(Check, Diagnostic)>,
    sources: Vec<Value>,
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
    fn position(&mut self, id: &str, path: &str, start: usize, end: usize) {
        self.source_map
            .entry(id.to_owned())
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .unwrap()
            .push(json!({"path":path,"start":start,"end":end}));
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
                let end = ctx.sources[0]["text"].as_str().map_or(0, str::len);
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
        sources: ctx.sources,
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
