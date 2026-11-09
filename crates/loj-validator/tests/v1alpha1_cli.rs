use serde_json::Value;
use std::{path::Path, process::Command};
fn run(args: &[&str]) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_loj-validator"))
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        output.status.code().unwrap(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}
#[test]
fn repository_and_manifest_selection_and_levels() {
    let output = Command::new(env!("CARGO_BIN_EXE_loj-validator"))
        .current_dir(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../vendor/loj-spec-v1alpha1/conformance/valid/T001"),
        )
        .args(["check", "law.yaml", "--profile=v1alpha1", "--format=json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    for path in [
        "vendor/loj-spec-v1alpha1/conformance/valid/T001",
        "vendor/loj-spec-v1alpha1/conformance/valid/T001/law.yaml",
    ] {
        let (code, r) = run(&["check", path, "--profile", "v1alpha1", "--format", "json"]);
        assert_eq!(code, 0);
        assert_eq!(r["status"], "Passed");
        assert!(r["ast"].is_object());
    }
    let (code, r) = run(&[
        "check",
        "vendor/loj-spec-v1alpha1/conformance/invalid/T042",
        "--level=L0",
        "--profile=v1alpha1",
        "--strict",
        "--format=json",
    ]);
    assert_eq!(code, 0);
    assert!(r["ast"].is_null());
    let (code, r) = run(&[
        "check",
        "vendor/loj-spec-v1alpha1/conformance/invalid/T042",
        "--profile=v1alpha1",
        "--format=json",
    ]);
    assert_eq!(code, 1);
    assert_eq!(r["status"], "Failed");
    assert!(r["ast"].is_null());
}
#[test]
fn configuration_errors_remain_json_and_legacy_is_compatible() {
    for args in [
        vec!["check", ".", "--level=L2", "--format=json"],
        vec!["check", ".", "--law-id=override", "--format=json"],
        vec![
            "check",
            "missing.yaml",
            "--profile=v1alpha1",
            "--format=json",
        ],
    ] {
        let (code, _) = run(&args);
        assert_eq!(code, 2);
    }
    let (code, r) = run(&[
        "check",
        "examples/simple-law.md",
        "--profile=legacy",
        "--structure-schema=schemas/law-of-japan.structure.yaml",
        "--format=json",
    ]);
    assert_eq!(code, 0);
    assert_eq!(r["status"], "passed");
}
