//! End-to-end tests of `loj-validator check`: both layers, exit codes, and
//! parseable JSON on validation failures and configuration errors.
// Test helpers outside `#[test]` functions may unwrap, and offsets read
// back from JSON are cast for slicing: a failure here fails the test.
#![allow(clippy::unwrap_used, clippy::cast_possible_truncation)]
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_SCHEMA: &str = "schemas/law-of-japan.structure.yaml";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Write a scratch file under Cargo's per-test temp directory.
fn scratch(name: &str, content: impl AsRef<[u8]>) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("loj-cli");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    path
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_loj-validator"))
        .current_dir(root())
        .args(args)
        .output()
        .unwrap();
    Run {
        code: out.status.code().expect("exited normally"),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

/// Run `check` with JSON output; stdout must be one JSON document, stderr empty.
fn check_json(input: &Path, schema: &Path, extra: &[&str]) -> (i32, Value) {
    let (input, schema) = (input.to_str().unwrap(), schema.to_str().unwrap());
    let mut args = vec![
        "check",
        input,
        "--structure-schema",
        schema,
        "--format",
        "json",
    ];
    args.extend_from_slice(extra);
    let r = run(&args);
    assert_eq!(r.stderr, "", "JSON mode must not log");
    let report: Value = serde_json::from_str(&r.stdout)
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}):\n{}", r.stdout));
    assert_eq!(
        report["exit_code"], r.code,
        "exit_code field matches process"
    );
    (r.code, report)
}

fn layer<'a>(report: &'a Value, name: &str) -> &'a Value {
    &report["layers"][name]
}

fn codes(layer: &Value) -> Vec<String> {
    let mut c: Vec<String> = layer["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap().to_owned())
        .collect();
    c.sort();
    c.dedup();
    c
}

fn default_schema() -> PathBuf {
    root().join(DEFAULT_SCHEMA)
}

fn permissive_schema() -> PathBuf {
    scratch("permissive.yaml", "{}\n")
}

#[test]
fn examples_pass_both_layers() {
    for example in ["examples/chaptered-law.md", "examples/simple-law.md"] {
        let (code, report) = check_json(&root().join(example), &default_schema(), &[]);
        assert_eq!(code, 0, "{example}: {report:#}");
        assert_eq!(report["status"], "passed");
        assert_eq!(layer(&report, "structure")["status"], "passed");
        let semantic = layer(&report, "semantic");
        assert_eq!(semantic["status"], "passed");
        assert_eq!(semantic["mapping_complete"], true);
        assert_eq!(semantic["profile"], "embedded:law-of-japan");
        assert_eq!(semantic["reference_coverage"]["complete"], true);
        assert_eq!(semantic["reference_coverage"]["total"], 1);
    }
    let r = run(&[
        "check",
        "examples/chaptered-law.md",
        "--structure-schema",
        DEFAULT_SCHEMA,
    ]);
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("[structure] passed"), "{}", r.stdout);
    assert!(r.stdout.contains("[semantic] passed"), "{}", r.stdout);
    assert!(r.stdout.contains("all references resolved"), "{}", r.stdout);
}

#[test]
fn chaptered_example_maps_expected_nodes() {
    let (_, report) = check_json(
        &root().join("examples/chaptered-law.md"),
        &default_schema(),
        &[],
    );
    let nodes = &layer(&report, "semantic")["nodes"];
    assert_eq!(nodes["Law"], 1);
    assert_eq!(nodes["Chapter"], 2);
    assert_eq!(nodes["Article"], 4);
    assert_eq!(nodes["Item"], 2);
    assert_eq!(nodes["Supplement"], 1);
}

#[test]
fn structure_failure_does_not_skip_semantic_layer() {
    let strict = scratch(
        "require-chapter.yaml",
        "sections:\n  - header: { pattern: '\\S' }\n    maxContains: 1\n    additionalSections: false\n    sections:\n      - header: { pattern: '^第.+章' }\n",
    );
    let (code, report) = check_json(&root().join("examples/simple-law.md"), &strict, &[]);
    assert_eq!(code, 1);
    assert_eq!(report["status"], "failed");
    let structure = layer(&report, "structure");
    assert_eq!(structure["status"], "failed");
    let diagnostics = structure["diagnostics"].as_array().unwrap();
    assert!(!diagnostics.is_empty());
    for d in diagnostics {
        assert_eq!(d["layer"], "structure");
        assert!(d["breadcrumb"].is_array());
        assert!(d["schema_pointer"].is_string());
        assert!(d["keyword"].is_string());
        // schematter has no source positions; none are invented.
        assert!(d.get("primary_span").is_none());
    }
    let semantic = layer(&report, "semantic");
    assert_eq!(semantic["status"], "passed");
    assert_eq!(semantic["reference_coverage"]["complete"], true);
}

#[test]
fn semantic_failures_are_caught_when_structure_is_permissive() {
    let cases = [
        (
            "missing-paragraph.md",
            "# 試験法\n\n## 第一条\n\n本文。\n\n## 第二条\n",
            &["SCHEMA_INVALID"][..],
        ),
        (
            "duplicate.md",
            "# 試験法\n\n## 第一条\n\n本文。\n\n## 第一条\n\n別の本文。\n",
            &["ID_DUPLICATE", "NUMBER_DUPLICATE"][..],
        ),
        (
            "missing-link.md",
            "# 試験法\n\n## 第一条\n\n[第九条](#第九条)を参照する。\n",
            &["REF_TARGET_MISSING"][..],
        ),
    ];
    for (name, text, expected) in cases {
        let (code, report) = check_json(&scratch(name, text), &permissive_schema(), &[]);
        assert_eq!(code, 1, "{name}");
        assert_eq!(layer(&report, "structure")["status"], "passed", "{name}");
        let semantic = layer(&report, "semantic");
        assert_eq!(semantic["status"], "failed", "{name}");
        assert_eq!(codes(semantic), expected, "{name}");
        for d in semantic["diagnostics"].as_array().unwrap() {
            assert_eq!(d["layer"], "semantic");
            for field in [
                "rule_id",
                "node_id",
                "primary_span",
                "related_spans",
                "stage",
            ] {
                assert!(d.get(field).is_some(), "{name}: {field} missing in {d}");
            }
        }
    }
}

#[test]
fn semantic_diagnostics_keep_source_spans() {
    let text = "# 試験法\n\n## 第一条\n\n[第九条](#第九条)を参照する。\n";
    let (_, report) = check_json(&scratch("span.md", text), &permissive_schema(), &[]);
    let d = &layer(&report, "semantic")["diagnostics"][0];
    assert_eq!(d["code"], "REF_TARGET_MISSING");
    assert_eq!(d["stage"], "reference");
    assert_eq!(d["rule_id"], "semantic/local-reference");
    let (s, e) = (
        d["primary_span"]["start_byte"].as_u64().unwrap() as usize,
        d["primary_span"]["end_byte"].as_u64().unwrap() as usize,
    );
    assert_eq!(&text[s..e], "[第九条](#第九条)");
    let input = scratch("span.md", text);
    let r = run(&[
        "check",
        input.to_str().unwrap(),
        "--structure-schema",
        permissive_schema().to_str().unwrap(),
    ]);
    assert_eq!(r.code, 1);
    assert!(r.stdout.contains("span.md:5:1 "), "{}", r.stdout);
}

#[test]
fn both_layers_report_their_own_failures() {
    let input = scratch("both.md", "# 試験法\n\n## 第一条\n\n本文。\n\n## 第二条\n");
    let (code, report) = check_json(&input, &default_schema(), &[]);
    assert_eq!(code, 1);
    assert_eq!(layer(&report, "structure")["status"], "failed");
    assert_eq!(layer(&report, "semantic")["status"], "failed");
    assert_eq!(codes(layer(&report, "semantic")), ["SCHEMA_INVALID"]);
    // No reference coverage claim for a schema-invalid AST.
    assert!(layer(&report, "semantic")["reference_coverage"].is_null());
}

#[test]
fn raw_heading_levels_are_checked_by_semantic_layer() {
    // schematter normalizes # → ##### to parent-child; the profile does not.
    let input = scratch("level-skip.md", "# 試験法\n\n##### 第一条\n\n本文。\n");
    let (code, report) = check_json(&input, &default_schema(), &[]);
    assert_eq!(code, 1);
    assert_eq!(layer(&report, "structure")["status"], "passed");
    let semantic = layer(&report, "semantic");
    assert_eq!(semantic["status"], "failed");
    assert_eq!(semantic["mapping_complete"], false);
    assert!(codes(semantic).contains(&"UNMAPPED".to_string()));
}

#[test]
fn out_of_scope_references_are_visible_not_passed() {
    let input = scratch(
        "external.md",
        "# 試験法\n\n## 第一条\n\n[外部](https://example.invalid/law)及び[第二条](other.md#第二条)を参照する。\n",
    );
    let (code, report) = check_json(&input, &default_schema(), &[]);
    assert_eq!(code, 0, "out-of-scope references are warnings: {report:#}");
    let semantic = layer(&report, "semantic");
    assert_eq!(semantic["status"], "passed");
    let c = &semantic["reference_coverage"];
    assert_eq!(c["total"], 2);
    assert_eq!(c["resolved"], 0);
    assert_eq!(c["out_of_scope"], 2);
    assert_eq!(c["complete"], false);
    let warnings: Vec<_> = semantic["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["severity"] == "warning")
        .collect();
    assert_eq!(warnings.len(), 2);
    assert!(warnings.iter().all(|d| d["code"] == "REF_OUT_OF_SCOPE"));
    assert_eq!(report["summary"]["warnings"], 2);

    let r = run(&[
        "check",
        input.to_str().unwrap(),
        "--structure-schema",
        DEFAULT_SCHEMA,
    ]);
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("INCOMPLETE"), "{}", r.stdout);
    assert!(r.stdout.contains("REF_OUT_OF_SCOPE"), "{}", r.stdout);
    assert!(
        !r.stdout.contains("all references resolved"),
        "{}",
        r.stdout
    );
}

#[test]
fn law_id_option_sets_semantic_identity() {
    let input = scratch(
        "dup-id.md",
        "# 試験法\n\n## 第一条\n\n本文。\n\n## 第一条\n\n別。\n",
    );
    let (_, report) = check_json(&input, &permissive_schema(), &["--law-id", "my-law"]);
    assert_eq!(report["law_id"], "my-law");
    let d = layer(&report, "semantic")["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "ID_DUPLICATE")
        .unwrap()
        .clone();
    assert_eq!(d["node_id"], "my-law/article/1");
}

#[test]
fn broken_structure_schemas_are_config_errors() {
    let input = root().join("examples/simple-law.md");
    let broken = [
        ("malformed.yaml", "sections: [ { header: \n"),
        ("not-a-mapping.yaml", "- 1\n- 2\n"),
        (
            "unknown-keyword.yaml",
            "sections:\n  - header: { const: X }\n    ordered: true\n",
        ),
        (
            "bad-regex.yaml",
            "sections:\n  - header: { pattern: '(' }\n",
        ),
    ];
    for (name, text) in broken {
        let (code, report) = check_json(&input, &scratch(name, text), &[]);
        assert_eq!(code, 2, "{name}: {report:#}");
        assert_eq!(report["status"], "error");
        let structure = layer(&report, "structure");
        assert_eq!(structure["status"], "error", "{name}");
        assert!(
            !structure["errors"].as_array().unwrap().is_empty(),
            "{name}"
        );
        // The other layer still ran.
        assert_eq!(layer(&report, "semantic")["status"], "passed", "{name}");
    }
    let missing = root().join("does-not-exist.yaml");
    let (code, report) = check_json(&input, &missing, &[]);
    assert_eq!(code, 2);
    assert_eq!(
        layer(&report, "structure")["errors"][0]["kind"],
        "structure-schema"
    );
    assert_eq!(layer(&report, "semantic")["status"], "passed");
}

#[test]
fn broken_profiles_are_config_errors() {
    let input = root().join("examples/simple-law.md");
    let mut bad_regex: Value = serde_json::from_str(law_semantic::LAW_OF_JAPAN_PROFILE).unwrap();
    bad_regex["headings"][2]["pattern"] = "(".into();
    let profiles = [
        ("malformed.json", "{ \"version\": \"0.1\", ".to_string()),
        ("array.json", "[]".to_string()),
        ("mode.json", "{\"paragraph_mode\":\"nope\",\"headings\":[],\"item_pattern\":\"(?P<number>a)(?P<body>b)\"}".to_string()),
        ("bad-regex.json", bad_regex.to_string()),
    ];
    for (name, text) in profiles {
        let profile = scratch(name, text);
        let (code, report) = check_json(
            &input,
            &default_schema(),
            &["--profile", profile.to_str().unwrap()],
        );
        assert_eq!(code, 2, "{name}: {report:#}");
        let semantic = layer(&report, "semantic");
        assert_eq!(semantic["status"], "error", "{name}");
        assert_eq!(semantic["errors"][0]["kind"], "profile", "{name}");
        assert_eq!(layer(&report, "structure")["status"], "passed", "{name}");
    }
    let (code, report) = check_json(&input, &default_schema(), &["--profile", "nope.json"]);
    assert_eq!(code, 2);
    assert_eq!(layer(&report, "semantic")["status"], "error");
}

#[test]
fn explicit_profile_file_is_used() {
    let profile = root().join("crates/law-semantic/profiles/paragraph-headings.json");
    let input = scratch(
        "heading-form.md",
        "# 試験法\n\n## 第一条\n\n### 第１項\n\n本文。\n",
    );
    let (code, report) = check_json(
        &input,
        &permissive_schema(),
        &["--profile", profile.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{report:#}");
    assert!(layer(&report, "semantic")["profile"]
        .as_str()
        .unwrap()
        .ends_with("paragraph-headings.json"));
}

#[test]
fn unreadable_input_is_a_tool_error() {
    let cases = [
        root().join("examples/missing.md"),
        scratch("latin1.md", b"# \xff\xfe\n".as_slice()),
    ];
    for input in cases {
        let (code, report) = check_json(&input, &default_schema(), &[]);
        assert_eq!(code, 2);
        assert_eq!(report["status"], "error");
        assert_eq!(report["errors"][0]["kind"], "input");
        assert_eq!(layer(&report, "structure")["status"], "not_run");
        assert_eq!(layer(&report, "semantic")["status"], "not_run");
    }
}

#[test]
fn usage_errors_exit_2() {
    let json_cases: [&[&str]; 4] = [
        &["check", "examples/simple-law.md", "--format", "json"],
        &[
            "check",
            "examples/simple-law.md",
            "--structure-schema",
            DEFAULT_SCHEMA,
            "--bogus",
            "x",
            "--format",
            "json",
        ],
        &[
            "check",
            "--format=json",
            "examples/simple-law.md",
            "--structure-schema",
            DEFAULT_SCHEMA,
            "--law-id=",
        ],
        &["frobnicate", "--format", "json"],
    ];
    for args in json_cases {
        let r = run(args);
        assert_eq!(r.code, 2, "{args:?}");
        assert_eq!(r.stderr, "");
        let report: Value = serde_json::from_str(&r.stdout).unwrap();
        assert_eq!(report["status"], "error");
        assert_eq!(report["errors"][0]["kind"], "usage");
    }
    for args in [
        &[
            "check",
            "examples/simple-law.md",
            "--structure-schema",
            DEFAULT_SCHEMA,
            "--format",
            "xml",
        ][..],
        &[][..],
    ] {
        let r = run(args);
        assert_eq!(r.code, 2, "{args:?}");
        assert_eq!(r.stdout, "");
        assert!(
            r.stderr.contains("usage: loj-validator check"),
            "{}",
            r.stderr
        );
    }
    let r = run(&["--help"]);
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("--structure-schema"));
}

#[test]
fn schema_failure_keeps_mapping_findings() {
    let input = scratch("empty-mapping-review.md", "");
    let (code, report) = check_json(&input, &permissive_schema(), &[]);
    assert_eq!(code, 1);
    let semantic = layer(&report, "semantic");
    assert_eq!(semantic["mapping_complete"], false);
    assert_eq!(semantic["reference_coverage"], Value::Null);
    let findings = semantic["diagnostics"].as_array().unwrap();
    assert!(findings.iter().any(|d| d["code"] == "SCHEMA_INVALID"));
    assert_eq!(
        findings
            .iter()
            .filter(|d| d["code"] == "LAW_TITLE_MISSING")
            .count(),
        1
    );
}
