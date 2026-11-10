// Test helpers outside `#[test]` functions may unwrap, and offsets read
// back from JSON are cast for slicing: a failure here fails the test.
#![allow(clippy::unwrap_used, clippy::cast_possible_truncation)]
use law_semantic::v1alpha1::{check, Level};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn outcome_problems(case: &Value, actual: &Value) -> Vec<String> {
    // T058 is the pinned unknown-version fixture (loj/v9). VAL-01 explicitly
    // calls unsupported versions unprocessable. This is a runner expectation,
    // not a parser exception; all other formal negatives must finish rejection.
    let expected = if case["id"] == "T058" {
        ("Error", 2, false)
    } else if case["success"] == true {
        ("Passed", 0, true)
    } else {
        ("Failed", 1, true)
    };
    let mut problems = Vec::new();
    if actual["status"] != expected.0
        || actual["exit_code"] != expected.1
        || actual["complete"] != expected.2
    {
        problems.push(format!(
            "Expected outcome {expected:?}, got status={} exit={} complete={}",
            actual["status"], actual["exit_code"], actual["complete"]
        ));
    }
    if case["success"] != true && !actual["ast"].is_null() {
        problems.push("Rejected input returned a complete AST".into());
    }
    if expected.0 == "Error" {
        let ds = actual["diagnostics"].as_array();
        if !ds.is_some_and(|ds| {
            ds.len() == 1
                && ds[0]["rule"] == "BASIC-01"
                && ds[0]["code"] == "loj/v1alpha1/unsupported-version"
                && ds[0]["file"] == "law.yaml"
        }) {
            problems.push("T058 requires the specific unsupported-version error, not arbitrary IO/internal errors".into());
        }
    }
    if actual["diagnostics"].as_array().is_some_and(|ds| {
        ds.iter()
            .any(|d| d["code"] == "loj/v1alpha1/specification-pending")
    }) {
        problems.push("Specification-pending outcome cannot pass conformance".into());
    }
    problems
}

#[test]
fn negative_outcomes_reject_incomplete_and_unexpected_errors() {
    let case = json!({"id":"T006","success":false});
    let good = json!({"status":"Failed","exit_code":1,"complete":true,"ast":null,"diagnostics":[{"rule":"DIR-02"}]});
    assert!(outcome_problems(&case, &good).is_empty());
    for (status, code) in [("Incomplete", 3), ("Error", 2), ("Passed", 0)] {
        let mut bad = good.clone();
        bad["status"] = json!(status);
        bad["exit_code"] = json!(code);
        bad["complete"] = json!(false);
        assert!(!outcome_problems(&case, &bad).is_empty());
    }
    let mut bad = good;
    bad["complete"] = json!(false);
    assert!(!outcome_problems(&case, &bad).is_empty());
}

#[test]
fn unknown_version_requires_expected_error_cause() {
    let case = json!({"id":"T058","success":false});
    let good = json!({"status":"Error","exit_code":2,"complete":false,"ast":null,"diagnostics":[{"rule":"BASIC-01","code":"loj/v1alpha1/unsupported-version","file":"law.yaml"}]});
    assert!(outcome_problems(&case, &good).is_empty());
    let mut bad = good.clone();
    bad["diagnostics"][0]["code"] = json!("internal");
    assert!(!outcome_problems(&case, &bad).is_empty());
    let mut bad = good;
    bad["diagnostics"]
        .as_array_mut()
        .unwrap()
        .push(json!({"rule":"MAN-02","code":"io"}));
    assert!(!outcome_problems(&case, &bad).is_empty());
}

#[test]
fn official_conformance() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/loj-spec-v1alpha1");
    assert_eq!(
        fs::read_to_string(root.join("COMMIT")).unwrap().trim(),
        law_semantic::v1alpha1::SPEC_COMMIT
    );
    let hashes: Value =
        serde_json::from_slice(&fs::read(root.join("SHA256SUMS.json")).unwrap()).unwrap();
    for (path, hash) in hashes.as_object().unwrap() {
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(root.join(path)).unwrap())),
            hash.as_str().unwrap(),
            "Fixed specification asset changed: {path}"
        );
    }
    let index: Value =
        serde_json::from_slice(&fs::read(root.join("conformance/manifest.json")).unwrap()).unwrap();
    let mut results = Vec::new();
    let mut failures = Vec::new();
    for case in index["cases"].as_array().unwrap() {
        if case["normative"] != true {
            results.push(json!({"id":case["id"],"result":"Informative: not executed"}));
            continue;
        }
        let expected: Value = serde_json::from_slice(
            &fs::read(root.join(case["expected"].as_str().unwrap())).unwrap(),
        )
        .unwrap();
        if case["id"] == "T058" {
            let input: Value = serde_json::from_slice(
                &fs::read(root.join(case["input"].as_str().unwrap())).unwrap(),
            )
            .unwrap();
            assert_eq!(input["schemaVersion"], "loj/v9");
            assert_eq!(case["level"], "L0");
            assert_eq!(case["success"], false);
            assert_eq!(expected["diagnostics"], json!([{"rule":"BASIC-01"}]));
        }
        let actual = check(
            &root.join(case["input"].as_str().unwrap()),
            if case["level"] == "L0" {
                Level::L0
            } else {
                Level::L1
            },
        );
        let mut problems = outcome_problems(case, &actual);
        if case["success"] == true {
            if actual["ast"] != expected["ast"] {
                problems.push("AST mismatch".into());
            }
            if let Some(positions) = expected["positions"].as_array() {
                for p in positions {
                    if !actual["positions"].as_array().unwrap().contains(p) {
                        problems.push(format!("Missing position: {p}"));
                    }
                }
            }
        } else {
            for d in expected["diagnostics"].as_array().unwrap() {
                if !actual["diagnostics"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|a| a["rule"] == d["rule"])
                {
                    problems.push(format!(
                        "Missing rule {}: {}",
                        d["rule"], actual["diagnostics"]
                    ));
                }
            }
        }
        if !problems.is_empty() {
            failures.push(format!("{}: {}", case["id"], problems.join("; ")));
        }
        results.push(json!({"id":case["id"],"level":case["level"],"result":if problems.is_empty(){"Passed"}else{"Failed"},"problems":problems,"actual":actual}));
    }
    if let Some(path) = std::env::var_os("LOJ_CONFORMANCE_REPORT") {
        fs::write(path, serde_json::to_vec_pretty(&results).unwrap()).unwrap();
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
