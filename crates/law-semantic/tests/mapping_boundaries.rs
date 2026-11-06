//! Mapping boundaries: fixed expectations, profile ambiguity and loss
//! boundaries. Port of `check_mapping_boundaries.py`; every mapped AST is also
//! validated, because completeness is a semantic constraint.
use law_semantic::{
    law_of_japan_profile, map_markdown, validate_document, PARAGRAPH_HEADINGS_PROFILE,
};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

fn codes(issues: &[Value]) -> BTreeSet<String> {
    issues
        .iter()
        .map(|d| d["code"].as_str().unwrap().to_owned())
        .collect()
}

fn canonical(v: &Value) -> Value {
    match v {
        Value::Array(a) => Value::Array(a.iter().map(canonical).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .filter(|(k, _)| !matches!(k.as_str(), "origins" | "display" | "implicit"))
                .map(|(k, x)| (k.clone(), canonical(x)))
                .collect::<Map<_, _>>(),
        ),
        _ => v.clone(),
    }
}

#[test]
fn mapping_boundaries() {
    let profile = law_of_japan_profile();
    let heading: Value = serde_json::from_str(PARAGRAPH_HEADINGS_PROFILE).unwrap();
    let mut ambiguous = profile.clone();
    let article = ambiguous["headings"][2].clone();
    ambiguous["headings"].as_array_mut().unwrap().push(article);
    let cases: Vec<(&str, &str, &Value, &[&str])> = vec![
        ("missing-last-body", "# 試験法\n\n## 第一条\n\n本文。\n\n## 第二条\n", &profile, &["SCHEMA_INVALID"]),
        ("unknown-heading", "# 試験法\n\n## 謎\n\n本文。\n", &profile, &["UNMAPPED", "INCOMPLETE"]),
        ("duplicate-id", "# 試験法\n\n## 第一条\n\n本文。\n\n## 第一条\n\n別の本文。\n", &profile, &["ID_DUPLICATE", "NUMBER_DUPLICATE"]),
        ("raw-level-skip", "# 試験法\n\n##### 第一条\n\n本文。\n", &profile, &["UNMAPPED", "INCOMPLETE"]),
        ("mixed-list", "# 試験法\n\n## 第一条\n\n本文。\n\n- 一　号。\n- 説明\n", &profile, &["AMBIGUOUS"]),
        ("after-list", "# 試験法\n\n## 第一条\n\n1. 本文。\n\n続きの所属が不明。\n", &profile, &["AMBIGUOUS", "ID_DUPLICATE", "NUMBER_DUPLICATE"]),
        ("inline-html", "# 試験法\n\n## 第一条\n\n本文<span>注</span>。\n", &profile, &["INCOMPLETE"]),
        ("item-inline-html", "# 試験法\n\n## 第一条\n\n本文。\n\n- 一　本文<span>注</span>。\n", &profile, &["INCOMPLETE"]),
        ("item-multiple-paragraphs", "# 試験法\n\n## 第一条\n\n本文。\n\n- 一　号の第一段落。\n\n  号の第二段落。\n", &profile, &[]),
        ("number-order-preserved", "# 試験法\n\n## 第一条\n\n1. 本文。\n3. 次の項。\n", &profile, &[]),
        ("invalid-han-number", "# 試験法\n\n## 第十十条\n\n本文。\n", &profile, &["NUMBER_INVALID", "INCOMPLETE"]),
        ("duplicate-law-heading", "# 試験法\n\n## 第一条\n\n本文。\n\n# 別法\n", &profile, &["LAW_TITLE_DUPLICATE"]),
        ("code-heading", "# 試験法\n\n## 第一条\n\n本文。\n\n```md\n## 第二条\n```\n", &profile, &[]),
        ("deleted-branch", "# 試験法\n\n## 第二条の二\n\n削除\n", &profile, &[]),
        ("two-profiles-list", "# 試験法\n\n## 第一条\n\n1. 本文。\n    - 一　号の本文。\n\n2. 続き。\n", &profile, &[]),
        ("two-profiles-heading", "# 試験法\n\n## 第一条\n\n### 第１項\n\n本文。\n\n- 一　号の本文。\n\n### 第２項\n\n続き。\n", &heading, &[]),
        ("ambiguous-rules", "# 試験法\n\n## 第一条\n\n本文。\n", &ambiguous, &["AMBIGUOUS", "INCOMPLETE"]),
    ];
    let mut failures = vec![];
    let mut asts = std::collections::HashMap::new();
    for (name, text, config, expected) in cases {
        let data = map_markdown(text, &format!("{name}.md"), "test-law", config).unwrap();
        let issues = validate_document(&data).unwrap();
        let observed = codes(&issues);
        let expected: BTreeSet<String> = expected.iter().map(|s| s.to_string()).collect();
        let law = &data["laws"][0];
        let extra_ok = match name {
            // Actual list markers, not the parser's implied sequential numbering.
            "number-order-preserved" => {
                law["children"][0]["children"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n["number"]["parts"].clone())
                    .collect::<Vec<_>>()
                    == vec![serde_json::json!([1]), serde_json::json!([3])]
            }
            "code-heading" => law["children"].as_array().unwrap().len() == 1,
            "deleted-branch" => law["children"][0]["number"]["parts"] == serde_json::json!([2, 2]),
            "item-multiple-paragraphs" => {
                law["children"][0]["children"][0]["children"][0]["content"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|b| b["text"].as_str().unwrap())
                    .collect::<Vec<_>>()
                    == ["号の第一段落。", "号の第二段落。"]
            }
            _ => true,
        };
        if observed != expected || !extra_ok {
            failures.push(format!(
                "{name}: expected {expected:?}, observed {observed:?}, extra checks {extra_ok}"
            ));
        }
        asts.insert(name, data);
    }
    assert!(failures.is_empty(), "{failures:#?}");
    assert_eq!(
        canonical(&asts["two-profiles-list"]["laws"]),
        canonical(&asts["two-profiles-heading"]["laws"]),
        "two profiles must yield the same semantic AST"
    );
}

#[test]
fn origins_point_at_source_bytes() {
    let text = "# 試験法\n\n## 第一条\n\n本文。\n";
    let data = map_markdown(text, "t.md", "t", &law_of_japan_profile()).unwrap();
    let article = &data["laws"][0]["children"][0];
    let span = &article["origins"][0];
    let (s, e) = (
        span["start_byte"].as_u64().unwrap() as usize,
        span["end_byte"].as_u64().unwrap() as usize,
    );
    assert_eq!(&text[s..e], "## 第一条\n\n本文。\n");
    assert_eq!(article["id"], "t/article/1");
    assert_eq!(data["sources"][0]["text"], text);
}
