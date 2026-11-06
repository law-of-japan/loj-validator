//! JSON Schema validation plus the relational checks the schema cannot express:
//! ID and scoped-number uniqueness, source hash and UTF-8 ranges, origin
//! containment, empty bodies, mixed supplements, deletion declarations, anchor
//! bindings and same-file reference resolution.
use crate::references;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

fn issue(out: &mut Vec<Value>, code: &str, message: String, node: &Value, related: Value) {
    out.push(json!({"code":code,"severity":"error","message":message,
        "rule_id":format!("semantic/{code}"),"node_id":node.get("id"),
        "primary_span":node["origins"].get(0),"related_spans":related}));
}

fn span_check(span: &Value, source: &HashMap<String, String>) -> Result<(), String> {
    let path = span["source"].as_str().unwrap();
    let text = source
        .get(path)
        .ok_or_else(|| format!("unknown source: {path}"))?;
    let start = span["start_byte"].as_u64().unwrap() as usize;
    let end = span["end_byte"].as_u64().unwrap() as usize;
    if start >= end
        || end > text.len()
        || !text.is_char_boundary(start)
        || !text.is_char_boundary(end)
    {
        return Err(format!("invalid UTF-8 interval [{start},{end}) in {path}"));
    }
    Ok(())
}

struct Checks {
    sources: HashMap<String, String>,
    ids: HashMap<String, Value>,
    duplicate_ids: std::collections::HashSet<String>,
    numbers: HashMap<String, Value>,
    issues: Vec<Value>,
}

impl Checks {
    fn node(&mut self, n: &Value, parent: Option<&Value>, scope: &str) {
        let kind = n["kind"].as_str().unwrap();
        let id = n["id"].as_str().unwrap();
        if let Some(prior) = self.ids.insert(id.into(), n["origins"].clone()) {
            self.duplicate_ids.insert(id.into());
            issue(
                &mut self.issues,
                "ID_DUPLICATE",
                format!("duplicate id: {id}"),
                n,
                prior,
            );
        }
        let scope = if kind == "Law" || kind == "Supplement" {
            id
        } else {
            scope
        };
        if !n["number"].is_null() {
            // Articles are unique across chapters within a main/supplement scope.
            let owner = if kind == "Article" {
                scope
            } else {
                parent.map(|p| p["id"].as_str().unwrap()).unwrap_or(scope)
            };
            let key = format!("{owner}|{kind}|{}", n["number"]["parts"]);
            if let Some(prior) = self.numbers.insert(key.clone(), n["origins"].clone()) {
                issue(
                    &mut self.issues,
                    "NUMBER_DUPLICATE",
                    format!("duplicate scoped number: {key}"),
                    n,
                    prior,
                );
            }
        }
        for span in n["origins"].as_array().unwrap() {
            if let Err(msg) = span_check(span, &self.sources) {
                issue(&mut self.issues, "SPAN_INVALID", msg, n, json!([]));
            }
            if let Some(p) = parent {
                let contains = p["origins"].as_array().unwrap().iter().any(|ps| {
                    ps["source"] == span["source"]
                        && ps["start_byte"].as_u64() <= span["start_byte"].as_u64()
                        && ps["end_byte"].as_u64() >= span["end_byte"].as_u64()
                });
                if !contains {
                    issue(
                        &mut self.issues,
                        "SPAN_OUTSIDE_PARENT",
                        "child origin is outside parent origins".into(),
                        n,
                        p["origins"].clone(),
                    );
                }
            }
        }
        for reference in n["references"].as_array().unwrap() {
            for span in reference["origins"].as_array().unwrap() {
                if let Err(msg) = span_check(span, &self.sources) {
                    issue(&mut self.issues, "SPAN_INVALID", msg, n, json!([]));
                }
                let contains = n["origins"].as_array().unwrap().iter().any(|ps| {
                    ps["source"] == span["source"]
                        && ps["start_byte"].as_u64() <= span["start_byte"].as_u64()
                        && ps["end_byte"].as_u64() >= span["end_byte"].as_u64()
                });
                if !contains {
                    issue(
                        &mut self.issues,
                        "REF_SPAN_OUTSIDE_NODE",
                        "reference source range is outside its owner".into(),
                        n,
                        json!([span]),
                    );
                }
            }
        }
        for block in n["content"].as_array().unwrap() {
            for span in block["origins"].as_array().unwrap() {
                if let Err(msg) = span_check(span, &self.sources) {
                    issue(&mut self.issues, "SPAN_INVALID", msg, n, json!([]));
                }
                let contains = n["origins"].as_array().unwrap().iter().any(|ps| {
                    ps["source"] == span["source"]
                        && ps["start_byte"].as_u64() <= span["start_byte"].as_u64()
                        && ps["end_byte"].as_u64() >= span["end_byte"].as_u64()
                });
                if !contains {
                    issue(
                        &mut self.issues,
                        "CONTENT_OUTSIDE_NODE",
                        "content origin is outside node origins".into(),
                        n,
                        json!([span]),
                    );
                }
            }
            if block["kind"] == "unsupported" {
                issue(
                    &mut self.issues,
                    "INCOMPLETE",
                    "unmapped content is preserved but not validated".into(),
                    n,
                    json!([]),
                );
            }
        }
        if (kind == "Paragraph" || kind == "Item")
            && n["content"]
                .as_array()
                .unwrap()
                .iter()
                .all(|b| b["text"].as_str().unwrap().trim().is_empty())
        {
            issue(
                &mut self.issues,
                "BODY_EMPTY",
                "node has no non-whitespace content".into(),
                n,
                json!([]),
            );
        }
        if kind == "Supplement" {
            let children = n["children"].as_array().unwrap();
            if children.iter().any(|c| c["kind"] == "Article")
                && children.iter().any(|c| c["kind"] == "Paragraph")
            {
                issue(
                    &mut self.issues,
                    "SUPPLEMENT_MIXED",
                    "supplement mixes articles and direct paragraphs".into(),
                    n,
                    json!([]),
                );
            }
        }
        if kind == "Article" && n["status"] == "deleted" {
            let blocks = n["content"].as_array().unwrap();
            if blocks.len() != 1 || blocks[0]["kind"] != "paragraph" || blocks[0]["text"] != "削除"
            {
                issue(
                    &mut self.issues,
                    "DELETION_INVALID",
                    "deleted article requires exactly the deletion declaration".into(),
                    n,
                    json!([]),
                );
            }
        }
        for child in n["children"].as_array().unwrap() {
            self.node(child, Some(n), scope);
        }
    }
}

/// Validate a semantic AST against `schema`, then run the relational checks.
///
/// Returns every error diagnostic (empty when valid). Schema violations are
/// reported as `SCHEMA_INVALID` and stop before the relational checks, exactly
/// as in the prototype. `Err` means the validator could not run: `schema` does
/// not compile, or `schema` accepted a document that does not satisfy the
/// built-in contract ([`crate::SEMANTIC_SCHEMA`]) the relational checks rely on.
pub fn validate(schema: &Value, data: &Value) -> Result<Vec<Value>, String> {
    let validator = jsonschema::options()
        .build(schema)
        .map_err(|e| e.to_string())?;
    let mut issues: Vec<Value> = validator.iter_errors(data).map(|e| {
        let path = e.instance_path().as_str().to_string();
        let mut ancestor = path.clone();
        let node = loop {
            let candidate = data.pointer(&ancestor).unwrap_or(&Value::Null);
            if candidate.get("id").is_some() { break candidate; }
            match ancestor.rfind('/') { Some(i)=>ancestor.truncate(i), None=>break &Value::Null }
        };
        json!({
        "code":"SCHEMA_INVALID", "severity":"error", "message":e.to_string(),
        "rule_id":format!("semantic/schema{}",e.schema_path().as_str()), "node_id":node.get("id"),
        "primary_span":node["origins"].get(0),"related_spans":[],"instance_path":path
    })}).collect();
    if !issues.is_empty() {
        return Ok(issues);
    }
    if !crate::conforms(data)? {
        return Err("document passed the supplied schema but not the built-in semantic contract required by relational checks".into());
    }
    let mut sources = HashMap::new();
    for src in data["sources"].as_array().unwrap() {
        let path = src["path"].as_str().unwrap();
        let text = src["text"].as_str().unwrap();
        if sources.insert(path.into(), text.into()).is_some() {
            issue(
                &mut issues,
                "SOURCE_DUPLICATE",
                format!("duplicate source: {path}"),
                &Value::Null,
                json!([]),
            );
        }
        let hash = format!("{:x}", Sha256::digest(text.as_bytes()));
        if hash != src["sha256"].as_str().unwrap() {
            issue(
                &mut issues,
                "SOURCE_HASH",
                format!("hash mismatch: {path}"),
                &Value::Null,
                json!([]),
            );
        }
    }
    let mut checks = Checks {
        sources,
        ids: HashMap::new(),
        duplicate_ids: std::collections::HashSet::new(),
        numbers: HashMap::new(),
        issues,
    };
    for law in data["laws"].as_array().unwrap() {
        checks.node(law, None, "");
    }
    for anchor in data["anchors"].as_array().unwrap() {
        for span in anchor["origins"].as_array().unwrap() {
            if let Err(msg) = span_check(span, &checks.sources) {
                issue(
                    &mut checks.issues,
                    "SPAN_INVALID",
                    msg,
                    &Value::Null,
                    json!([span]),
                );
            }
            if let Some(id) = anchor["node_id"].as_str() {
                let contains =
                    checks
                        .ids
                        .get(id)
                        .and_then(Value::as_array)
                        .is_some_and(|origins| {
                            origins.iter().any(|ps| {
                                ps["source"] == span["source"]
                                    && ps["start_byte"].as_u64() <= span["start_byte"].as_u64()
                                    && ps["end_byte"].as_u64() >= span["end_byte"].as_u64()
                            })
                        });
                if !contains && !checks.duplicate_ids.contains(id) {
                    issue(
                        &mut checks.issues,
                        "ANCHOR_BINDING_INVALID",
                        format!("anchor not within bound node {id}"),
                        &Value::Null,
                        json!([span]),
                    );
                }
            }
        }
    }
    for d in data["diagnostics"].as_array().unwrap() {
        if d["severity"] == "error" && d["rule_id"] != "semantic/local-reference" {
            checks.issues.push(d.clone());
        }
    }
    checks.issues.extend(
        references::resolve(&mut data.clone())
            .into_iter()
            .filter(|d| d["severity"] == "error"),
    );
    Ok(checks.issues)
}
