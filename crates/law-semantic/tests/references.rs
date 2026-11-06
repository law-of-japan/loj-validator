//! Paired Markdown forms test actual links, not separately entered reference
//! metadata. Port of `check_references.py`.
use law_semantic::{
    law_of_japan_profile, map_markdown, reference_coverage, resolve_references, validate_document,
    PARAGRAPH_HEADINGS_PROFILE,
};
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashMap};

fn nodes(n: &Value, out: &mut Vec<Value>) {
    out.push(n.clone());
    for c in n["children"].as_array().unwrap() {
        nodes(c, out);
    }
}

fn refs(data: &Value) -> Vec<Value> {
    let mut all = vec![];
    for law in data["laws"].as_array().unwrap() {
        nodes(law, &mut all);
    }
    all.iter()
        .flat_map(|n| n["references"].as_array().unwrap().clone())
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

fn codes(issues: &[Value], severity: Option<&str>) -> BTreeSet<String> {
    issues
        .iter()
        .filter(|d| severity.is_none_or(|s| d["severity"] == s))
        .map(|d| d["code"].as_str().unwrap().to_owned())
        .collect()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

type Spec = (
    &'static str,
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
    bool,
);

const SPECS: &[Spec] = &[
    ("valid", "第二条", "#第二条", &[], &[], true),
    (
        "encoded",
        "第二条",
        "#%E7%AC%AC%E4%BA%8C%E6%9D%A1",
        &[],
        &[],
        true,
    ),
    (
        "missing",
        "第二条",
        "#第九十九条",
        &["REF_TARGET_MISSING"],
        &[],
        false,
    ),
    (
        "wrong-type",
        "法令",
        "#試験法",
        &["REF_TYPE_MISMATCH"],
        &[],
        false,
    ),
    (
        "wrong-number",
        "第三条",
        "#第二条",
        &["REF_LABEL_MISMATCH"],
        &[],
        false,
    ),
    ("free-label", "参照先", "#第二条", &[], &[], true),
    (
        "bad-percent",
        "参照先",
        "#%ZZ",
        &["REF_FRAGMENT_INVALID"],
        &[],
        false,
    ),
    (
        "bad-utf8",
        "参照先",
        "#%FF",
        &["REF_FRAGMENT_INVALID"],
        &[],
        false,
    ),
    (
        "external",
        "第二条",
        "https://example.invalid/law",
        &[],
        &["REF_OUT_OF_SCOPE"],
        false,
    ),
    (
        "cross-file",
        "第二条",
        "other.md#第二条",
        &[],
        &["REF_OUT_OF_SCOPE"],
        false,
    ),
    (
        "duplicate-id",
        "第二条",
        "#第二条",
        &["ID_DUPLICATE", "NUMBER_DUPLICATE", "REF_AMBIGUOUS"],
        &[],
        false,
    ),
    (
        "missing-paragraph",
        "第二条",
        "#第二条",
        &["SCHEMA_INVALID"],
        &[],
        false,
    ),
];

fn document(form: &str, name: &str, label: &str, uri: &str) -> String {
    let paragraph = |s: &str| {
        if form == "list" {
            format!("1. {s}")
        } else {
            format!("### 第１項\n\n{s}")
        }
    };
    let body = format!("[{label}]({uri})を参照する。");
    let mut text = format!(
        "# 試験法\n\n## 第一条\n\n{}\n\n## 第二条\n\n",
        paragraph(&body)
    );
    if name != "missing-paragraph" {
        text += &format!("{}\n", paragraph("保存する。"));
    }
    if name == "duplicate-id" {
        text += &format!("\n## 第二条\n\n{}\n", paragraph("別の条。"));
    }
    text
}

#[test]
fn references_in_two_markdown_forms() {
    let heading: Value = serde_json::from_str(PARAGRAPH_HEADINGS_PROFILE).unwrap();
    let list = law_of_japan_profile();
    let mut asts: HashMap<(&str, &str), Value> = HashMap::new();
    let mut failures = vec![];
    for (form, profile) in [("list", &list), ("heading", &heading)] {
        for &(name, label, uri, expected, warnings, resolved) in SPECS {
            let text = document(form, name, label, uri);
            let path = format!("{form}-{name}.md");
            let mapped = map_markdown(&text, &path, "test-law", profile).unwrap();
            let mut data = mapped.clone();
            if name != "missing-paragraph" {
                resolve_references(&mut data).unwrap();
            } else {
                assert!(resolve_references(&mut data.clone()).is_err());
            }
            let issues = validate_document(&data).unwrap();
            let observed = codes(&issues, None);
            let observed_warnings = codes(data["diagnostics"].as_array().unwrap(), Some("warning"));
            let rs = refs(&data);
            let mut ok = observed == set(expected)
                && observed_warnings == set(warnings)
                && issues.is_empty() == expected.is_empty()
                && rs.len() == 1;
            if ok && name != "missing-paragraph" {
                ok = (rs[0]["resolution"] == "resolved") == resolved;
            }
            if ok && matches!(name, "external" | "cross-file") {
                let c = reference_coverage(&data).unwrap();
                ok = c["out_of_scope"] == 1
                    && c["complete"] == false
                    && c["warnings"].as_array().unwrap().len() == 1;
            }
            if ok && resolved {
                ok = rs[0]["target_id"] == "test-law/article/2";
            }
            // Exact source range proves diagnostics point at the Markdown link.
            let span = rs[0]["origins"][0].clone();
            let (s, e) = (
                span["start_byte"].as_u64().unwrap() as usize,
                span["end_byte"].as_u64().unwrap() as usize,
            );
            ok = ok && text[s..e] == format!("[{label}]({uri})");
            for d in &issues {
                if d["code"].as_str().unwrap().starts_with("REF_") {
                    ok = ok && d["primary_span"] == span;
                }
            }
            if !ok {
                failures.push(format!(
                    "{form}-{name}: {observed:?} warnings {observed_warnings:?}"
                ));
            }
            asts.insert((form, name), data);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");

    for &(name, ..) in SPECS {
        // The empty-paragraph fixture differs structurally; compare rejection only.
        if name == "missing-paragraph" {
            continue;
        }
        assert_eq!(
            canonical(&asts[&("list", name)]["laws"]),
            canonical(&asts[&("heading", name)]["laws"]),
            "paired-{name}"
        );
    }

    // Model-level anchor collision and namespace tests, distinct from Markdown cases.
    let mut base = asts[&("list", "valid")].clone();
    let anchor = base["anchors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "第二条")
        .unwrap()
        .clone();
    base["anchors"].as_array_mut().unwrap().push(anchor);
    let issues = validate_document(&base).unwrap();
    assert_eq!(
        codes(&issues, None),
        set(&["REF_AMBIGUOUS"]),
        "duplicate-anchor"
    );

    let mut base = asts[&("list", "valid")].clone();
    for a in base["anchors"].as_array_mut().unwrap() {
        if a["name"] == "第二条" {
            a["origins"][0]["source"] = json!("other.md");
        }
    }
    let issues = validate_document(&base).unwrap();
    assert!(!issues.is_empty(), "source-isolation");
    assert!(
        codes(&issues, None).contains("REF_TARGET_MISSING"),
        "source-isolation"
    );
}

#[test]
fn supplied_resolution_flags_are_not_trusted() {
    let text = document("list", "missing", "第二条", "#第九十九条");
    let mut data = map_markdown(&text, "t.md", "test-law", &law_of_japan_profile()).unwrap();
    for law in data["laws"].as_array_mut().unwrap() {
        let r = &mut law["children"][0]["children"][0]["references"][0];
        r["resolution"] = json!("resolved");
        r["target_id"] = json!("test-law/article/2");
    }
    let issues = validate_document(&data).unwrap();
    assert!(codes(&issues, None).contains("REF_TARGET_MISSING"));
    let coverage = reference_coverage(&data).unwrap();
    assert_eq!(coverage["resolved"], 0);
    assert_eq!(coverage["complete"], false);
}
