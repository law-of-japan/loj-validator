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
fn integer_valued_float_offsets_do_not_panic() {
    let mut data = map_markdown(
        "# 試験法\n\n## 第一条\n\n本文。\n",
        "t.md",
        "t",
        &law_of_japan_profile(),
    )
    .unwrap();
    assert!(validate_document(&data).unwrap().is_empty());
    data["laws"][0]["origins"][0]["start_byte"] = json!(0.0);
    assert_eq!(conforms(&data), Ok(true));
    assert!(validate_document(&data).unwrap().is_empty());
}

fn float_offsets(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if matches!(key.as_str(), "start_byte" | "end_byte") {
                    *value = serde_json::from_str(&format!("{value}e0")).unwrap();
                } else {
                    float_offsets(value);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(float_offsets),
        _ => {}
    }
}

#[test]
fn float_offsets_preserve_containment_and_bounds() {
    let base = map_markdown(
        "# 試験法\n\n## 第一条\n\n[第一条](#第一条)を参照する。\n",
        "t.md",
        "t",
        &law_of_japan_profile(),
    )
    .unwrap();
    assert!(validate_document(&base).unwrap().is_empty());
    assert!(!base["anchors"].as_array().unwrap().is_empty());
    let mut floats = base.clone();
    float_offsets(&mut floats);
    floats["laws"][0]["origins"][0]["start_byte"] = json!(-0.0);
    assert_eq!(conforms(&floats), Ok(true));
    assert!(validate_document(&floats).unwrap().is_empty());
    let mut outside = base.clone();
    outside["laws"][0]["origins"][0]["end_byte"] = json!(1);
    floats["laws"][0]["origins"][0]["end_byte"] = json!(1.0);
    let expected = codes(&validate_document(&outside).unwrap());
    assert!(expected.contains(&"SPAN_OUTSIDE_PARENT".into()));
    assert_eq!(codes(&validate_document(&floats).unwrap()), expected);
    floats["laws"][0]["origins"][0]["end_byte"] = json!(9007199254740991.0);
    assert_eq!(conforms(&floats), Ok(true));
    assert!(codes(&validate_document(&floats).unwrap()).contains(&"SPAN_INVALID".into()));
}

#[test]
fn malformed_legacy_fields_stop_before_relational_unwraps() {
    let base = map_markdown(
        "# 試験法\n\n## 第一条\n\n本文。\n",
        "t.md",
        "t",
        &law_of_japan_profile(),
    )
    .unwrap();
    for pointer in [
        "/sources",
        "/sources/0/path",
        "/sources/0/text",
        "/sources/0/sha256",
        "/laws",
        "/laws/0/id",
        "/laws/0/kind",
        "/laws/0/origins",
        "/laws/0/origins/0/source",
        "/laws/0/origins/0/start_byte",
        "/laws/0/children",
        "/laws/0/content",
        "/laws/0/references",
        "/anchors",
        "/diagnostics",
    ] {
        let mut bad = base.clone();
        *bad.pointer_mut(pointer).unwrap() = Value::Null;
        assert_eq!(
            codes(&validate_document(&bad).unwrap()),
            ["SCHEMA_INVALID"],
            "{pointer}"
        );
        assert!(validate(&json!({}), &bad).is_err(), "{pointer}");
    }
    for value in [json!(-1), json!(0.5), json!(1e30), json!("0")] {
        let mut bad = base.clone();
        bad["laws"][0]["origins"][0]["start_byte"] = value;
        assert_eq!(codes(&validate_document(&bad).unwrap()), ["SCHEMA_INVALID"]);
    }
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
