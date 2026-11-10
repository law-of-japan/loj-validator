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
//!
//! # Public API
//!
//! Three pipelines share this crate, and what is public differs for each.
//!
//! - **Legacy** (the functions of this module, and [`mapping`], [`number`],
//!   [`validation`], [`references`]): names, signatures and JSON are kept
//!   compatible.
//! - **loj/v1alpha1** ([`v1alpha1::check`] and [`v1alpha1::Level`]): kept
//!   compatible, including the JSON it returns.
//! - **Next specification** ([`checks`] and the report types of [`common`];
//!   [`v1alpha2::check`] for one package; [`fixref::parse`] for the syntax of
//!   a reference): follows the specification and may change with it.
//!
//! The modules behind [`checks::run`] (the registry, repositories, the change
//! and reference checks, the git wrapper) are private: a check is reached
//! through its input kind, not through its module.
pub(crate) mod change;
pub mod checks;
pub mod common;
pub mod fixref;
pub(crate) mod git;
pub mod mapping;
pub mod number;
pub(crate) mod rebuild;
pub mod references;
pub(crate) mod registry;
pub(crate) mod repository;
pub mod v1alpha1;
pub mod v1alpha2;
pub mod validation;
pub(crate) mod xref;

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
#[must_use]
pub fn law_of_japan_profile() -> Value {
    serde_json::from_str(LAW_OF_JAPAN_PROFILE).expect("embedded profile is JSON")
}

/// Whether `data` satisfies [`SEMANTIC_SCHEMA`].
///
/// # Errors
///
/// The embedded schema does not compile (a defect of this crate).
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
///
/// # Errors
///
/// The profile is not usable; see [`mapping::map`].
pub fn map_markdown(
    source: &str,
    path: &str,
    law_id: &str,
    profile: &Value,
) -> Result<Value, String> {
    mapping::map(source, path, law_id, profile)
}

/// [`validate`] against the embedded [`SEMANTIC_SCHEMA`].
///
/// # Errors
///
/// The validator could not run; see [`validate`]. Problems of the document
/// itself are in the returned diagnostics.
pub fn validate_document(data: &Value) -> Result<Vec<Value>, String> {
    validate(semantic_schema(), data)
}

/// Resolve same-file references in place.
///
/// Recomputes every reference's `resolution`/`target_id` (supplied flags are
/// never trusted), replaces the AST's previous `semantic/local-reference`
/// diagnostics with the new ones, and returns them (errors and warnings).
///
/// # Errors
///
/// `data` does not satisfy [`SEMANTIC_SCHEMA`]. It is then left unchanged.
pub fn resolve_references(data: &mut Value) -> Result<Vec<Value>, String> {
    require_conforming(data)?;
    let diagnostics = references::resolve(data);
    // The schema makes `diagnostics` an array.
    if let Some(list) = data["diagnostics"].as_array_mut() {
        list.retain(|d| d["rule_id"] != "semantic/local-reference");
        list.extend(diagnostics.iter().cloned());
    }
    Ok(diagnostics)
}

/// Reference coverage summary under the same-file fragment policy.
///
/// `complete` is true only when every reference resolved; out-of-scope
/// references (external, cross-file) are counted and listed as warnings.
///
/// # Errors
///
/// `data` does not satisfy [`SEMANTIC_SCHEMA`].
pub fn reference_coverage(data: &Value) -> Result<Value, String> {
    require_conforming(data)?;
    Ok(references::coverage(data))
}

/// Normalize a displayed law number (`二の二`, `１２ノ３`) into positive parts.
///
/// # Errors
///
/// The text is not a law number; see [`number::normalize`].
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
