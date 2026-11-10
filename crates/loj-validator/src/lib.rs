//! Runs the two validation layers over one Markdown file and builds a report.
//!
//! - **structure**: the upstream `schematter-lib` document-schema validator,
//!   over the normalized heading tree and blocks.
//! - **semantic**: `law-semantic` mapping to a semantic AST, then schema,
//!   relational and same-file reference checks.
//!
//! Both layers run whenever the input is readable, independently of each
//! other's outcome. A configuration error in one layer does not stop the other.
pub mod next;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::Path;

/// Law identity used when `--law-id` is not given.
pub const DEFAULT_LAW_ID: &str = "law";
/// Profile name reported when the embedded law-of-japan profile is used.
pub const EMBEDDED_PROFILE: &str = "embedded:law-of-japan";
/// Structure engine identification; must match the pinned dependency.
pub const STRUCTURE_ENGINE: &str = "schematter-lib 0.2.0";

/// Outcome of a layer or of the whole check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Passed,
    Failed,
    /// A tool or configuration error prevented the check.
    Error,
    /// Not attempted because the input could not be read.
    NotRun,
}

impl Status {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Passed => "passed",
            Status::Failed => "failed",
            Status::Error => "error",
            Status::NotRun => "not_run",
        }
    }
    /// Process exit code: 0 passed, 1 validation failure, 2 tool/config error.
    #[must_use]
    pub fn exit_code(self) -> i32 {
        match self {
            Status::Passed => 0,
            Status::Failed => 1,
            Status::Error | Status::NotRun => 2,
        }
    }
}

/// Result of one layer.
#[derive(Clone, Debug)]
pub struct Layer {
    pub status: Status,
    /// Validation findings, each tagged with `"layer"`.
    pub diagnostics: Vec<Value>,
    /// Tool/configuration errors (`kind`, `message`, optional `pointer`).
    pub errors: Vec<Value>,
    /// Layer-specific fields added to the report object.
    pub extra: Map<String, Value>,
}

impl Layer {
    fn new(status: Status) -> Self {
        Layer {
            status,
            diagnostics: vec![],
            errors: vec![],
            extra: Map::new(),
        }
    }
    fn error(kind: &str, message: String) -> Self {
        let mut layer = Layer::new(Status::Error);
        layer.errors.push(json!({"kind":kind,"message":message}));
        layer
    }
    fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("status".into(), json!(self.status.as_str()));
        out.extend(self.extra.clone());
        out.insert("diagnostics".into(), json!(self.diagnostics));
        out.insert("errors".into(), json!(self.errors));
        Value::Object(out)
    }
}

/// Structure layer: validate `markdown` against a schematter schema source.
#[must_use]
pub fn check_structure(markdown: &str, schema_source: &str) -> Layer {
    match schematter_lib::validate(markdown, schema_source) {
        Err(errors) => {
            let mut layer = Layer::new(Status::Error);
            layer.errors = errors
                .into_iter()
                .map(|e| json!({"kind":"structure-schema","pointer":e.pointer,"message":e.message}))
                .collect();
            layer
        }
        Ok(violations) => {
            let mut layer = Layer::new(if violations.is_empty() {
                Status::Passed
            } else {
                Status::Failed
            });
            // schematter reports no source positions; none are added here.
            layer.diagnostics = violations
                .iter()
                .map(|v| {
                    json!({"layer":"structure","code":"STRUCTURE_VIOLATION","severity":"error",
                        "keyword":v.keyword,"message":v.message,"hint":v.hint,
                        "breadcrumb":v.breadcrumb.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
                        "schema_pointer":v.schema_pointer})
                })
                .collect();
            layer
        }
    }
}

/// Which part of the semantic layer produced a diagnostic, from its `rule_id`.
fn stage(rule_id: &str) -> &'static str {
    if rule_id.starts_with("mapping/") {
        "mapping"
    } else if rule_id.starts_with("semantic/schema") {
        "schema"
    } else if rule_id == "semantic/local-reference" {
        "reference"
    } else {
        "relational"
    }
}

fn tag_semantic(mut d: Value) -> Value {
    let stage = stage(d["rule_id"].as_str().unwrap_or(""));
    if let Some(o) = d.as_object_mut() {
        o.insert("layer".into(), json!("semantic"));
        o.insert("stage".into(), json!(stage));
    }
    d
}

fn count_nodes(n: &Value, counts: &mut Map<String, Value>) {
    if let Some(kind) = n["kind"].as_str() {
        let c = counts.get(kind).and_then(Value::as_u64).unwrap_or(0);
        counts.insert(kind.into(), json!(c + 1));
    }
    for child in n["children"].as_array().into_iter().flatten() {
        count_nodes(child, counts);
    }
}

/// Semantic layer: map `markdown` with `profile`, then validate the AST.
///
/// Passes only when the AST satisfies the semantic schema, the relational
/// checks and reference resolution, and mapping left nothing unmapped.
pub fn check_semantic(markdown: &str, source_path: &str, law_id: &str, profile: &Value) -> Layer {
    let ast = match law_semantic::map_markdown(markdown, source_path, law_id, profile) {
        Ok(ast) => ast,
        Err(e) => return Layer::error("profile", e),
    };
    let mut issues = match law_semantic::validate_document(&ast) {
        Ok(issues) => issues,
        Err(e) => return Layer::error("semantic-validator", e),
    };
    // Schema failure stops relational checks, but must not hide mapping findings.
    let mapping_diagnostics = ast["diagnostics"].as_array().cloned().unwrap_or_default();
    let mapping_complete = !mapping_diagnostics.iter().any(|d| d["severity"] == "error");
    for diagnostic in mapping_diagnostics {
        if !issues.contains(&diagnostic) {
            issues.push(diagnostic);
        }
    }
    let mut layer = Layer::new(if issues.is_empty() {
        Status::Passed
    } else {
        Status::Failed
    });
    layer.diagnostics = issues.into_iter().map(tag_semantic).collect();
    // Coverage needs a schema-valid AST, as in the prototype; otherwise null.
    let coverage = law_semantic::reference_coverage(&ast).ok();
    if let Some(c) = &coverage {
        for w in c["warnings"].as_array().into_iter().flatten() {
            layer.diagnostics.push(tag_semantic(w.clone()));
        }
    }
    let mut nodes = Map::new();
    for law in ast["laws"].as_array().into_iter().flatten() {
        count_nodes(law, &mut nodes);
    }
    layer
        .extra
        .insert("ast_version".into(), ast["version"].clone());
    layer
        .extra
        .insert("mapping_complete".into(), json!(mapping_complete));
    layer.extra.insert("nodes".into(), Value::Object(nodes));
    layer
        .extra
        .insert("reference_coverage".into(), coverage.unwrap_or(Value::Null));
    layer
}

/// Inputs of one `check` run.
#[derive(Clone, Debug)]
pub struct CheckOptions<'a> {
    pub input: &'a Path,
    pub structure_schema: &'a Path,
    /// `None` uses the embedded law-of-japan profile.
    pub profile: Option<&'a Path>,
    pub law_id: &'a str,
}

/// A finished report and the source it refers to (for line/column rendering).
#[derive(Clone, Debug)]
pub struct Report {
    pub status: Status,
    pub json: Value,
    pub source: Option<String>,
}

impl Report {
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        self.status.exit_code()
    }
}

fn display_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn read_text(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", display_path(path)))?;
    String::from_utf8(bytes).map_err(|_| format!("{}: not valid UTF-8", display_path(path)))
}

/// Report for a failure before any layer ran (usage, unreadable input, panic).
#[must_use]
pub fn error_report(kind: &str, message: &str, input: Option<&str>) -> Report {
    let json = json!({"tool":"loj-validator","version":env!("CARGO_PKG_VERSION"),
        "status":"error","exit_code":2,"input":input,
        "errors":[{"kind":kind,"message":message}],
        "layers":{"structure":{"status":"not_run","diagnostics":[],"errors":[]},
                  "semantic":{"status":"not_run","diagnostics":[],"errors":[]}},
        "summary":{"errors":0,"warnings":0}});
    Report {
        status: Status::Error,
        json,
        source: None,
    }
}

/// Run both layers. Never panics on bad files or configuration.
pub fn check(options: &CheckOptions) -> Report {
    let input = display_path(options.input);
    let markdown = match read_text(options.input) {
        Ok(text) => text,
        Err(e) => return error_report("input", &e, Some(&input)),
    };

    let mut structure = match read_text(options.structure_schema) {
        Ok(schema) => check_structure(&markdown, &schema),
        Err(e) => Layer::error("structure-schema", e),
    };
    structure
        .extra
        .insert("engine".into(), json!(STRUCTURE_ENGINE));
    structure.extra.insert(
        "schema".into(),
        json!(display_path(options.structure_schema)),
    );

    let profile = match options.profile {
        None => Ok(law_semantic::law_of_japan_profile()),
        Some(path) => read_text(path).and_then(|text| {
            serde_json::from_str::<Value>(&text)
                .map_err(|e| format!("{}: invalid JSON: {e}", display_path(path)))
        }),
    };
    let mut semantic = match profile {
        Ok(profile) => check_semantic(&markdown, &input, options.law_id, &profile),
        Err(e) => Layer::error("profile", e),
    };
    semantic.extra.insert(
        "profile".into(),
        json!(options
            .profile
            .map(display_path)
            .unwrap_or_else(|| EMBEDDED_PROFILE.into())),
    );

    let layers = [&structure, &semantic];
    let status = if layers.iter().any(|l| l.status == Status::Error) {
        Status::Error
    } else if layers.iter().any(|l| l.status == Status::Failed) {
        Status::Failed
    } else {
        Status::Passed
    };
    let all = || layers.iter().flat_map(|l| l.diagnostics.iter());
    let errors = all().filter(|d| d["severity"] == "error").count();
    let warnings = all().filter(|d| d["severity"] == "warning").count();
    let json = json!({"tool":"loj-validator","version":env!("CARGO_PKG_VERSION"),
        "status":status.as_str(),"exit_code":status.exit_code(),
        "input":input,"law_id":options.law_id,"errors":[],
        "layers":{"structure":structure.to_json(),"semantic":semantic.to_json()},
        "summary":{"errors":errors,"warnings":warnings}});
    Report {
        status,
        json,
        source: Some(markdown),
    }
}

/// 1-based line and column (in characters) of a byte offset.
fn line_col(source: &str, byte: usize) -> Option<(usize, usize)> {
    if byte > source.len() || !source.is_char_boundary(byte) {
        return None;
    }
    let before = &source[..byte];
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    Some((line, column))
}

fn render_diagnostic(d: &Value, source: Option<&str>, out: &mut String) {
    let severity = d["severity"].as_str().unwrap_or("error");
    let code = d["code"].as_str().unwrap_or("?");
    let message = d["message"].as_str().unwrap_or("");
    if d["layer"] == "structure" {
        let crumbs: Vec<&str> = d["breadcrumb"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let at = if crumbs.is_empty() {
            "(document)".to_string()
        } else {
            crumbs.join(" › ")
        };
        out.push_str(&format!(
            "  {severity} {code} [{}] {at}: {message} (schema {})\n",
            d["keyword"].as_str().unwrap_or(""),
            d["schema_pointer"].as_str().unwrap_or("")
        ));
        if let Some(hint) = d["hint"].as_str() {
            out.push_str(&format!("    hint: {hint}\n"));
        }
        return;
    }
    let span = &d["primary_span"];
    let at = match (span["source"].as_str(), span["start_byte"].as_u64()) {
        (Some(file), Some(start)) => {
            let pos = source
                .and_then(|s| line_col(s, usize::try_from(start).ok()?))
                .map(|(l, c)| format!(":{l}:{c}"))
                .unwrap_or_default();
            format!("{file}{pos} ")
        }
        _ => String::new(),
    };
    let node = d["node_id"]
        .as_str()
        .map(|n| format!(" node={n}"))
        .unwrap_or_default();
    out.push_str(&format!(
        "  {severity} {code} {at}({}{node}): {message}\n",
        d["rule_id"].as_str().unwrap_or("")
    ));
}

/// Human-readable rendering of a report.
#[must_use]
pub fn render_text(report: &Report) -> String {
    let j = &report.json;
    let mut out = format!(
        "loj-validator: {} (exit {})\n",
        j["status"].as_str().unwrap_or("error").to_uppercase(),
        report.exit_code()
    );
    if let Some(input) = j["input"].as_str() {
        out.push_str(&format!(
            "input: {input}  law-id: {}\n",
            j["law_id"].as_str().unwrap_or("-")
        ));
    }
    for e in j["errors"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "error ({}): {}\n",
            e["kind"].as_str().unwrap_or(""),
            e["message"].as_str().unwrap_or("")
        ));
    }
    for name in ["structure", "semantic"] {
        let layer = &j["layers"][name];
        let detail = match name {
            "structure" => format!(
                "{}, schema {}",
                layer["engine"].as_str().unwrap_or(STRUCTURE_ENGINE),
                layer["schema"].as_str().unwrap_or("-")
            ),
            _ => format!("profile {}", layer["profile"].as_str().unwrap_or("-")),
        };
        out.push_str(&format!(
            "[{name}] {} ({detail})\n",
            layer["status"].as_str().unwrap_or("not_run")
        ));
        for e in layer["errors"].as_array().into_iter().flatten() {
            let pointer = e["pointer"]
                .as_str()
                .filter(|p| !p.is_empty())
                .map(|p| format!(" at {p}"))
                .unwrap_or_default();
            out.push_str(&format!(
                "  config-error ({}){pointer}: {}\n",
                e["kind"].as_str().unwrap_or(""),
                e["message"].as_str().unwrap_or("")
            ));
        }
        for d in layer["diagnostics"].as_array().into_iter().flatten() {
            render_diagnostic(d, report.source.as_deref(), &mut out);
        }
    }
    let semantic = &j["layers"]["semantic"];
    let c = &semantic["reference_coverage"];
    if c.is_object() {
        let complete = c["complete"] == true;
        out.push_str(&format!(
            "references ({}): total {}, resolved {}, out of scope {}, failed {} — {}\n",
            c["policy"].as_str().unwrap_or(""),
            c["total"],
            c["resolved"],
            c["out_of_scope"],
            c["failed"],
            if complete {
                "all references resolved"
            } else {
                "INCOMPLETE: unresolved or out-of-scope references were not verified"
            }
        ));
    } else if semantic["status"] == "failed" {
        out.push_str("references: not evaluated (semantic AST is not schema-valid)\n");
    } else {
        out.push_str("references: not evaluated (semantic layer did not run)\n");
    }
    if semantic["mapping_complete"] == false {
        out.push_str("mapping: INCOMPLETE (unmapped or unsupported content present)\n");
    }
    out
}
