//! Independent loj/v1alpha1 pipeline; legacy APIs and JSON remain unchanged.
mod directives;
mod manifest;
mod markdown;
mod number;
mod syntax;
use serde_json::{json, Value};
use std::path::Path;

pub const SPEC_COMMIT: &str = "6295fd7769a09eeb0da56bcc6ff8c404ec391db6";
pub const MANIFEST_SCHEMA: &str =
    include_str!("../../../../vendor/loj-spec/schemas/v1alpha1/manifest.schema.json");
pub const AST_SCHEMA: &str =
    include_str!("../../../../vendor/loj-spec/schemas/v1alpha1/ast.schema.json");
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    L0,
    L1,
}

#[derive(Default)]
pub(super) struct Context {
    diagnostics: Vec<Value>,
    sources: Vec<Value>,
    positions: Vec<Value>,
    source_map: serde_json::Map<String, Value>,
    error: bool,
    incomplete: bool,
}
impl Context {
    fn diag(&mut self, rule: &str, message: &str, path: &str, start: usize, end: usize) {
        self.diagnostics.push(json!({"rule":rule,"code":"loj/v1alpha1/violation","severity":"error","message":message,"file":path,"span":{"start":start,"end":end}}));
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

/// Validate a manifest path or a repository directory. Never returns a complete AST on failure.
pub fn check(input: &Path, level: Level) -> Value {
    let mut ctx = Context::default();
    let path = if input.is_dir() {
        input.join("law.yaml")
    } else {
        input.to_owned()
    };
    let loaded = manifest::load(&path, &mut ctx);
    let mut ast = None;
    if let Some((manifest, documents)) = loaded {
        if level == Level::L1 {
            if manifest["unsupportedFeatures"]
                .as_array()
                .is_some_and(|a| !a.is_empty())
            {
                ctx.diag(
                    "BASIC-03",
                    "Declared unsupported legal structure",
                    "law.yaml",
                    0,
                    ctx.sources[0]["text"].as_str().map_or(0, str::len),
                );
            }
            let tree = syntax::build(&manifest, &documents, &mut ctx);
            let schema: Value = serde_json::from_str(AST_SCHEMA).expect("embedded schema");
            let validator = jsonschema::options()
                .should_validate_formats(true)
                .build(&schema)
                .expect("embedded schema");
            if !validator.is_valid(&tree) {
                ctx.diag(
                    "AST-01",
                    "Constructed tree fails AST schema",
                    "law.yaml",
                    0,
                    0,
                );
            }
            ast = Some(tree);
        } else {
            for (_, source, _) in &documents {
                for _ in pulldown_cmark::Parser::new(source) {}
            }
        }
    }
    let (status, exit) = if ctx.error {
        ("Error", 2)
    } else if ctx.incomplete {
        ("Incomplete", 3)
    } else if !ctx.diagnostics.is_empty() {
        ("Failed", 1)
    } else {
        ("Passed", 0)
    };
    json!({"profile":"loj/v1alpha1","specCommit":SPEC_COMMIT,"level":format!("{level:?}"),"status":status,"exit_code":exit,"complete":!ctx.error && !ctx.incomplete,"ast":if status == "Passed" {ast} else {None},"diagnostics":ctx.diagnostics,"sources":ctx.sources,"positions":ctx.positions,"sourceMap":ctx.source_map})
}
