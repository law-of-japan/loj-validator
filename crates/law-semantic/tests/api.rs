//! Public API behaviour and robustness against bad input and configuration.
// Test helpers outside `#[test]` functions may unwrap, and offsets read
// back from JSON are cast for slicing: a failure here fails the test.
#![allow(clippy::unwrap_used, clippy::cast_possible_truncation)]
use law_semantic::{
    conforms, law_of_japan_profile, map_markdown, mapping, normalize_number, reference_coverage,
    resolve_references, validate, validate_document, EGOV_DERIVED_BARE_PROFILE,
};
use serde_json::{json, Value};

fn codes(issues: &[Value]) -> Vec<String> {
    let mut c: Vec<String> = issues
        .iter()
        .map(|d| d["code"].as_str().unwrap().to_owned())
        .collect();
    c.sort();
    c.dedup();
    c
}

#[test]
fn invalid_profiles_are_errors_not_panics() {
    let base = law_of_japan_profile();
    let mut variants: Vec<(&str, Value)> = vec![
        ("not an object", json!([1, 2])),
        ("empty object", json!({})),
    ];
    let mut v = base.clone();
    v["paragraph_mode"] = json!("nope");
    variants.push(("unsupported mode", v));
    let mut v = base.clone();
    v["headings"][1]["pattern"] = json!("(");
    variants.push(("bad regex", v));
    let mut v = base.clone();
    v["headings"][1]["kind"] = json!("Section");
    variants.push(("unknown kind", v));
    let mut v = base.clone();
    v["headings"][1]["levels"] = json!([7]);
    variants.push(("bad level", v));
    let mut v = base.clone();
    v["item_pattern"] = json!("^(?P<number>x)$");
    variants.push(("item body capture missing", v));
    let mut v = base.clone();
    v["anchor_mode"] = json!("gitlab");
    variants.push(("anchor mode", v));
    let mut v = base.clone();
    v["reference_target_kinds"] = json!([]);
    variants.push(("reference kinds", v));
    let mut v = base.clone();
    v["paragraph_mode"] = json!("bare");
    variants.push(("bare without paragraph_pattern", v));
    for (name, profile) in variants {
        assert!(
            map_markdown("# 試験法\n\n## 第一条\n\n本文。\n", "t.md", "t", &profile).is_err(),
            "{name}"
        );
    }
}

#[test]
fn optional_capture_groups_do_not_panic() {
    let mut profile = law_of_japan_profile();
    profile["item_pattern"] = json!("^(?P<number>[一二三])?(?P<body>本文.*)$");
    let data = map_markdown(
        "# 試験法\n\n## 第一条\n\n本文。\n\n- 本文の号。\n",
        "t.md",
        "t",
        &profile,
    )
    .unwrap();
    let issues = validate_document(&data).unwrap();
    assert!(codes(&issues).contains(&"NUMBER_INVALID".to_string()));
}

#[test]
fn permissive_schema_does_not_reach_relational_checks() {
    // `{}` accepts anything; the relational checks still need the real contract.
    let result = validate(&json!({}), &json!({"laws": "not an array"}));
    assert!(result.is_err());
    assert!(validate(&json!({"type": 1}), &json!({})).is_err());
}

#[test]
fn checked_entry_points_reject_non_conforming_input() {
    let mut bad = json!({"version": "0.1"});
    assert_eq!(conforms(&bad), Ok(false));
    assert!(resolve_references(&mut bad).is_err());
    assert!(reference_coverage(&bad).is_err());
    assert_eq!(codes(&validate_document(&bad).unwrap()), ["SCHEMA_INVALID"]);
}

#[test]
fn resolve_replaces_local_reference_diagnostics() {
    let text = "# 試験法\n\n## 第一条\n\n[外部](https://example.invalid/)を参照する。\n";
    let mut data = map_markdown(text, "t.md", "t", &law_of_japan_profile()).unwrap();
    let first = resolve_references(&mut data).unwrap();
    let second = resolve_references(&mut data).unwrap();
    assert_eq!(first, second);
    let local: Vec<_> = data["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["rule_id"] == "semantic/local-reference")
        .collect();
    assert_eq!(local.len(), 1);
    assert_eq!(local[0]["code"], "REF_OUT_OF_SCOPE");
    assert_eq!(local[0]["severity"], "warning");
    assert!(validate_document(&data).unwrap().is_empty());
}

#[test]
fn bare_profile_maps_unmarked_paragraphs_and_items() {
    let profile: Value = serde_json::from_str(EGOV_DERIVED_BARE_PROFILE).unwrap();
    let text = "# 試験法\n\n## 第一条\n\n本文。\n\n２　第二項の本文。\n\n一　第一号\n";
    let data = map_markdown(text, "t.md", "t", &profile).unwrap();
    assert!(validate_document(&data).unwrap().is_empty());
    let paragraphs = data["laws"][0]["children"][0]["children"]
        .as_array()
        .unwrap();
    let numbers: Vec<_> = paragraphs
        .iter()
        .map(|p| p["number"]["parts"].clone())
        .collect();
    assert_eq!(numbers, [json!([1]), json!([2])]);
    assert_eq!(paragraphs[0]["number"]["implicit"], true);
    assert_eq!(paragraphs[1]["children"][0]["kind"], "Item");
}

#[test]
fn merge_combines_files_of_one_law() {
    let profile = law_of_japan_profile();
    let a = map_markdown(
        "# 試験法\n\n## 第一条\n\n本文。\n",
        "a.md",
        "law-x",
        &profile,
    )
    .unwrap();
    let b = map_markdown(
        "# 試験法\n\n## 第二条\n\n本文。\n",
        "b.md",
        "law-x",
        &profile,
    )
    .unwrap();
    let merged = mapping::merge(&[a.clone(), b]).unwrap();
    assert_eq!(merged["laws"].as_array().unwrap().len(), 1);
    assert_eq!(merged["sources"].as_array().unwrap().len(), 2);
    assert_eq!(merged["laws"][0]["children"].as_array().unwrap().len(), 2);
    assert!(validate_document(&merged).unwrap().is_empty());

    let c = map_markdown("# 別法\n\n## 第三条\n\n本文。\n", "c.md", "law-x", &profile).unwrap();
    let mismatch = mapping::merge(&[a, c]).unwrap();
    assert!(codes(&validate_document(&mismatch).unwrap()).contains(&"LAW_TITLE_MISMATCH".into()));
    assert!(mapping::merge(&[json!({})]).is_err());
}

#[test]
fn number_normalization_is_exposed() {
    assert_eq!(
        normalize_number("第"),
        Err("unsupported numeral: 第".into())
    );
    assert_eq!(normalize_number("十二の三").unwrap(), [12, 3]);
}
