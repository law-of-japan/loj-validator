//! Semantic layer for Japanese law Markdown.
//!
//! Two parts, kept separate:
//!
//! - **Mapping** ([`mapping`], [`number`]): a declarative profile turns Markdown
//!   into a semantic AST (Law / Chapter / Article / Paragraph / Item /
//!   Supplement) with byte-range origins, heading anchors and link references.
//!   Mapping problems are recorded in the AST's `diagnostics`, never dropped.
//! - **Validation** ([`validation`], [`references`]): the AST is checked against
//!   the JSON Schema [`SEMANTIC_SCHEMA`] and then by relational checks (unique
//!   IDs and scoped numbers, source hashes and spans, deletion and supplement
//!   rules) and same-file reference resolution.
//!
//! Functions that walk an AST require it to satisfy [`SEMANTIC_SCHEMA`]; the
//! public entry points check this and return `Err` instead of panicking.
pub mod checks;
pub mod common;
pub mod mapping;
pub mod number;
pub mod references;
pub mod v1alpha1;
pub mod v1alpha2;
pub mod validation;

use serde_json::Value;
use std::sync::OnceLock;

pub use validation::validate;

/// JSON Schema (2020-12) of the semantic AST, version `0.1`.
pub const SEMANTIC_SCHEMA: &str = include_str!("../schema.json");
/// Default mapping profile: law-of-japan Markdown (list-form paragraphs).
pub const LAW_OF_JAPAN_PROFILE: &str = include_str!("../profiles/law-of-japan.json");
/// Mapping profile with `### 第１項` paragraph headings.
pub const PARAGRAPH_HEADINGS_PROFILE: &str = include_str!("../profiles/paragraph-headings.json");
/// Mapping profile with bare `２　…` paragraphs and non-list `一　…` items.
pub const EGOV_DERIVED_BARE_PROFILE: &str = include_str!("../profiles/egov-derived-bare.json");

/// Parsed [`SEMANTIC_SCHEMA`].
pub fn semantic_schema() -> &'static Value {
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    SCHEMA.get_or_init(|| serde_json::from_str(SEMANTIC_SCHEMA).expect("embedded schema is JSON"))
}

/// Parsed [`LAW_OF_JAPAN_PROFILE`].
pub fn law_of_japan_profile() -> Value {
    serde_json::from_str(LAW_OF_JAPAN_PROFILE).expect("embedded profile is JSON")
}

/// Whether `data` satisfies [`SEMANTIC_SCHEMA`].
pub fn conforms(data: &Value) -> Result<bool, String> {
    let validator = jsonschema::options()
        .build(semantic_schema())
        .map_err(|e| e.to_string())?;
    Ok(validator.is_valid(data))
}

fn require_conforming(data: &Value) -> Result<(), String> {
    if conforms(data)? {
        Ok(())
    } else {
        Err("input does not conform to semantic schema".into())
    }
}

/// Map one Markdown source into a semantic AST using `profile`.
///
/// `Err` is a profile/configuration error. Mapping problems in the Markdown
/// itself are returned inside the AST's `diagnostics`.
pub fn map_markdown(
    source: &str,
    path: &str,
    law_id: &str,
    profile: &Value,
) -> Result<Value, String> {
    mapping::map(source, path, law_id, profile)
}

/// [`validate`] against the embedded [`SEMANTIC_SCHEMA`].
pub fn validate_document(data: &Value) -> Result<Vec<Value>, String> {
    validate(semantic_schema(), data)
}

/// Resolve same-file references in place.
///
/// Recomputes every reference's `resolution`/`target_id` (supplied flags are
/// never trusted), replaces the AST's previous `semantic/local-reference`
/// diagnostics with the new ones, and returns them (errors and warnings).
pub fn resolve_references(data: &mut Value) -> Result<Vec<Value>, String> {
    require_conforming(data)?;
    let diagnostics = references::resolve(data);
    let list = data["diagnostics"].as_array_mut().unwrap();
    list.retain(|d| d["rule_id"] != "semantic/local-reference");
    list.extend(diagnostics.iter().cloned());
    Ok(diagnostics)
}

/// Reference coverage summary under the same-file fragment policy.
///
/// `complete` is true only when every reference resolved; out-of-scope
/// references (external, cross-file) are counted and listed as warnings.
pub fn reference_coverage(data: &Value) -> Result<Value, String> {
    require_conforming(data)?;
    Ok(references::coverage(data))
}

/// Normalize a displayed law number (`二の二`, `１２ノ３`) into positive parts.
pub fn normalize_number(input: &str) -> Result<Vec<u64>, String> {
    number::normalize(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_assets_parse() {
        assert!(jsonschema::options().build(semantic_schema()).is_ok());
        for profile in [
            LAW_OF_JAPAN_PROFILE,
            PARAGRAPH_HEADINGS_PROFILE,
            EGOV_DERIVED_BARE_PROFILE,
        ] {
            let profile: Value = serde_json::from_str(profile).unwrap();
            assert!(map_markdown("# 試験法\n", "t.md", "t", &profile).is_ok());
        }
    }
}
