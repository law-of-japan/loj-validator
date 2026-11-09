use serde_json::Value;
use std::{path::Path, process::Command};

fn run(args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_loj-validator"))
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .args(args)
        .output()
        .unwrap();
    (
        output.status.code().unwrap(),
        String::from_utf8(output.stdout).unwrap(),
    )
}

fn json(args: &[&str]) -> (i32, Value) {
    let mut a = args.to_vec();
    a.push("--format=json");
    let (code, out) = run(&a);
    (code, serde_json::from_str(&out).unwrap())
}

const PKG: &str = "vendor/loj-spec/conformance/v1alpha2/valid/pkg-110";

#[test]
fn package_report_has_the_report_schema_shape() {
    let (code, r) = json(&["check", PKG]);
    assert_eq!(code, 0, "{r}");
    assert_eq!(r["results"][0]["check"], "L0");
    assert_eq!(r["results"][1]["check"], "L1");
    assert_eq!(r["results"][1]["result"], "pass");
    assert!(r["diagnostics"].as_array().unwrap().is_empty());
    let schema: Value = serde_json::from_str(include_str!(
        "../../../vendor/loj-spec/schemas/common/v1alpha1/report.schema.json"
    ))
    .unwrap();
    assert!(jsonschema::is_valid(&schema, &r));
    let (code, r) = json(&["check", &format!("{PKG}/law.yaml"), "--level", "L0"]);
    assert_eq!(code, 0);
    assert_eq!(r["results"].as_array().unwrap().len(), 1);
}

#[test]
fn failures_incomplete_and_reserved_checks() {
    let (code, r) = json(&[
        "check",
        "vendor/loj-spec/conformance/v1alpha2/invalid/pkg-118",
        "--level=L1",
    ]);
    assert_eq!(code, 1);
    assert_eq!(r["results"][0]["result"], "fail");
    assert!(r["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["rule"] == "TABLE-03" && d["schemaVersion"] == "loj/v1alpha2"));
    // A loj/v1alpha1 package is not read as loj/v1alpha2 (VAL-04).
    let (code, r) = json(&[
        "check",
        "vendor/loj-spec/conformance/v1alpha2/invalid/pkg-124",
    ]);
    assert_eq!(code, 3);
    assert_eq!(r["results"][0]["result"], "incomplete");
    for reserved in [
        "--level=L2",
        "--check=L3/sourceMatch",
        "--category=clauseReference",
    ] {
        let (code, _) = run(&["check", PKG, reserved]);
        assert_eq!(code, 2, "{reserved}");
    }
    // A category selection on a package is reported, not silently passed.
    let (code, r) = json(&["check", PKG, "--category=evidence"]);
    assert_eq!(code, 3);
    assert_eq!(r["results"][0]["result"], "notApplicable");
}

#[test]
fn repository_registry_and_records() {
    let base = "vendor/loj-spec/conformance";
    let (code, r) = json(&[
        "check",
        &format!("{base}/loj-history/v1alpha1/valid/hist-001/repo"),
        "--level=L3",
    ]);
    assert_eq!(code, 0, "{r}");
    assert_eq!(r["results"].as_array().unwrap().len(), 2);
    let (code, r) = json(&[
        "check",
        &format!("{base}/loj-glyphs/v1alpha1/invalid/glyph-009/repo"),
        "--level=L1",
        "--category=evidence",
    ]);
    assert_eq!(code, 1, "{r}");
    let (code, r) = json(&[
        "check",
        &format!("{base}/loj-registry/v1alpha1/invalid/reg-003/registry"),
    ]);
    assert_eq!(code, 1);
    assert_eq!(r["results"][0]["check"], "registry");
    let (code, r) = json(&[
        "check",
        &format!("--registry={base}/loj-registry/v1alpha1/valid/reg-001/registry"),
        &format!("--repository={base}/loj-history/v1alpha1/valid/hist-004/repo"),
        "--category=timeConsistency",
    ]);
    assert_eq!(code, 3, "{r}");
    assert_eq!(r["references"][0]["result"], "notFetched");
}
