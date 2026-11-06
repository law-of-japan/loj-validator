//! The prototype's 28 semantic-AST cases (written without Markdown), with
//! exact diagnostic-code sets. Port of `check_cases.py`.
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[test]
fn semantic_ast_cases() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("cases/manifest.json")).unwrap())
            .unwrap();
    let cases = manifest.as_array().unwrap();
    assert_eq!(cases.len(), 28);
    let mut failures = vec![];
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let data: Value = serde_json::from_str(
            &fs::read_to_string(dir.join(case["file"].as_str().unwrap())).unwrap(),
        )
        .unwrap();
        let issues = law_semantic::validate_document(&data).unwrap();
        let observed: BTreeSet<&str> = issues.iter().map(|d| d["code"].as_str().unwrap()).collect();
        let expected: BTreeSet<&str> = case["expected_codes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap())
            .collect();
        let valid = issues.is_empty();
        let expected_valid = case["exit_code"] == 0;
        if observed != expected || valid != expected_valid {
            failures.push(format!(
                "{name}: expected {expected:?}, observed {observed:?}"
            ));
        }
        // Coverage is only defined for schema-valid documents, as in the prototype.
        let schema_valid = !observed.contains("SCHEMA_INVALID");
        assert_eq!(
            law_semantic::reference_coverage(&data).is_ok(),
            schema_valid,
            "{name}"
        );
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
