//! Formal conformance of the next specification (vendored loj-spec, see
//! vendor/loj-spec/COMMIT): one test per suite. Expected results are never
//! adjusted to the implementation (CONF-01).
use law_semantic::common::{Check, Report, ResultValue, SPEC_COMMIT};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, path::PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/loj-spec")
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn vendored_specification_is_pinned() {
    let root = root();
    assert_eq!(
        fs::read_to_string(root.join("COMMIT")).unwrap().trim(),
        SPEC_COMMIT
    );
    let hashes = read_json(&root.join("SHA256SUMS.json"));
    assert!(hashes.as_object().unwrap().len() > 1000);
    for (path, hash) in hashes.as_object().unwrap() {
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(root.join(path)).unwrap())),
            hash.as_str().unwrap(),
            "Fixed specification asset changed: {path}"
        );
    }
}

/// The checks a case is judged on: its expected results, else its own check.
fn selected(case: &Value, expected: &Value) -> Vec<Check> {
    let mut out = Vec::new();
    let mut push = |check: &str, category: Option<&str>| {
        let label = match category {
            Some(c) => format!("{check}/{c}"),
            None => check.to_owned(),
        };
        for c in Check::parse(&label).unwrap() {
            if !out.contains(&c) {
                out.push(c);
            }
        }
    };
    if let Some(results) = expected["results"].as_array() {
        for r in results {
            push(r["check"].as_str().unwrap(), r["category"].as_str());
        }
    }
    push(case["check"].as_str().unwrap(), case["category"].as_str());
    out
}

struct Actual {
    report: Report,
    ast: Option<Value>,
    positions: Vec<Value>,
}

fn run(case: &Value, checks: &[Check]) -> Actual {
    let root = root();
    let input = &case["input"];
    match input["kind"].as_str().unwrap() {
        "package" => {
            let r =
                law_semantic::v1alpha2::check(&root.join(input["path"].as_str().unwrap()), checks);
            Actual {
                report: r.report,
                ast: r.ast,
                positions: r.positions,
            }
        }
        "repository" => Actual {
            report: law_semantic::checks::run(
                &law_semantic::checks::Input::Repository(
                    root.join(input["path"].as_str().unwrap()),
                ),
                checks,
            ),
            ast: None,
            positions: Vec::new(),
        },
        "registry" => Actual {
            report: law_semantic::checks::run(
                &law_semantic::checks::Input::Registry(root.join(input["path"].as_str().unwrap())),
                checks,
            ),
            ast: None,
            positions: Vec::new(),
        },
        "records" => Actual {
            report: law_semantic::checks::run(
                &law_semantic::checks::Input::Records {
                    registry: root.join(input["registry"].as_str().unwrap()),
                    repositories: input["repositories"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|p| root.join(p.as_str().unwrap()))
                        .collect(),
                },
                checks,
            ),
            ast: None,
            positions: Vec::new(),
        },
        "change" => Actual {
            report: law_semantic::checks::run(
                &law_semantic::checks::Input::Change {
                    bundle: root.join(input["bundle"].as_str().unwrap()),
                    base: input["base"].as_str().unwrap().into(),
                    head: input["head"].as_str().unwrap().into(),
                },
                checks,
            ),
            ast: None,
            positions: Vec::new(),
        },
        "reference" => Actual {
            report: law_semantic::checks::run(
                &law_semantic::checks::Input::Reference {
                    bundle: root.join(input["bundle"].as_str().unwrap()),
                    registry: root.join(input["registry"].as_str().unwrap()),
                    reference: input["reference"].as_str().unwrap().into(),
                },
                checks,
            ),
            ast: None,
            positions: Vec::new(),
        },
        other => panic!("unknown input kind: {other}"),
    }
}

fn result_value(s: &str) -> ResultValue {
    match s {
        "pass" => ResultValue::Pass,
        "fail" => ResultValue::Fail,
        "notApplicable" => ResultValue::NotApplicable,
        _ => ResultValue::Incomplete,
    }
}

fn problems(case: &Value, expected: &Value, actual: &Actual) -> Vec<String> {
    let mut problems = Vec::new();
    let report = &actual.report;
    let json = report.to_json();
    let diagnostics = json["diagnostics"].as_array().unwrap();
    let has = |rule: &Value, version: &Value| {
        diagnostics
            .iter()
            .any(|d| d["rule"] == *rule && d["schemaVersion"] == *version && d["class"] == "error")
    };
    let mut expected_results = Vec::new();
    if let Some(results) = expected["results"].as_array() {
        for r in results {
            let label = match r["category"].as_str() {
                Some(c) => format!("{}/{c}", r["check"].as_str().unwrap()),
                None => r["check"].as_str().unwrap().to_owned(),
            };
            let check = Check::parse(&label).unwrap()[0];
            expected_results.push(check);
            let want = result_value(r["result"].as_str().unwrap());
            if report.result(check) != Some(want) {
                problems.push(format!(
                    "{label}: expected {}, got {:?}",
                    want.as_str(),
                    report.result(check).map(ResultValue::as_str)
                ));
            }
        }
    }
    let own = Check::parse(&match case["category"].as_str() {
        Some(c) => format!("{}/{c}", case["check"].as_str().unwrap()),
        None => case["check"].as_str().unwrap().to_owned(),
    })
    .unwrap();
    if case["success"] == true {
        for c in &own {
            if !expected_results.contains(c) && report.result(*c) != Some(ResultValue::Pass) {
                problems.push(format!(
                    "{}: expected pass, got {:?}",
                    c.label(),
                    report.result(*c).map(ResultValue::as_str)
                ));
            }
        }
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d["class"] == "error")
            .collect();
        if !errors.is_empty() {
            problems.push(format!("unexpected error diagnostics: {errors:?}"));
        }
        if let Some(ast) = expected.get("ast") {
            if actual.ast.as_ref() != Some(ast) {
                problems.push(format!(
                    "AST mismatch: {}",
                    serde_json::to_string(&actual.ast).unwrap()
                ));
            }
        }
        if let Some(positions) = expected["positions"].as_array() {
            for p in positions {
                if !actual.positions.contains(p) {
                    problems.push(format!("missing position: {p}"));
                }
            }
        }
    } else {
        if actual.ast.is_some() {
            problems.push("rejected input returned a complete AST".into());
        }
        if expected_results.is_empty() {
            for c in &own {
                if matches!(
                    report.result(*c),
                    None | Some(ResultValue::Pass) | Some(ResultValue::NotApplicable)
                ) {
                    problems.push(format!(
                        "{}: a failing case must not pass, got {:?}",
                        c.label(),
                        report.result(*c).map(ResultValue::as_str)
                    ));
                }
            }
        }
    }
    if let Some(ds) = expected["diagnostics"].as_array() {
        for d in ds {
            if !has(&d["rule"], &d["schemaVersion"]) {
                problems.push(format!(
                    "missing {} {} diagnostic; got {}",
                    d["rule"],
                    d["schemaVersion"],
                    serde_json::to_string(diagnostics).unwrap()
                ));
            }
        }
    }
    // References: every expected entry is reported (its given fields match).
    // CONF-01: listed references must be reported; extra ones are allowed.
    if let Some(want) = expected["references"].as_array() {
        let got = json["references"].as_array().cloned().unwrap_or_default();
        for w in want {
            let found = got.iter().any(|g| {
                w.as_object()
                    .unwrap()
                    .iter()
                    .all(|(k, v)| g.get(k) == Some(v))
            });
            if !found {
                problems.push(format!("missing reference {w}; got {got:?}"));
            }
        }
    }
    if let Some(want) = expected.get("resolution") {
        if json.get("resolution") != Some(want) {
            problems.push(format!(
                "resolution: expected {want}, got {:?}",
                json.get("resolution")
            ));
        }
    }
    problems
}

fn suite(name: &str) {
    suite_kinds(name, &[]);
}

/// Run the cases of a suite whose input kind is listed (all when empty).
fn suite_kinds(name: &str, kinds: &[&str]) {
    let root = root();
    let index = read_json(&root.join(format!("conformance/{name}/manifest.json")));
    let report_schema = law_semantic::common::validator(
        include_str!("../../../vendor/loj-spec/schemas/common/v1alpha1/report.schema.json"),
        &[],
    );
    let mut results = Vec::new();
    let mut failures = Vec::new();
    let (mut passed, mut informative) = (0, 0);
    for case in index["cases"].as_array().unwrap() {
        if !kinds.is_empty() && !kinds.contains(&case["input"]["kind"].as_str().unwrap()) {
            continue;
        }
        if case["normative"] != true {
            informative += 1;
            results.push(json!({"id":case["id"],"result":"informative: not counted"}));
            continue;
        }
        let expected = read_json(&root.join(case["expected"].as_str().unwrap()));
        let checks = selected(case, &expected);
        let actual = run(case, &checks);
        let mut problems = problems(case, &expected, &actual);
        if !report_schema.is_valid(&actual.report.to_json()) {
            problems.push("report does not match report.schema.json".into());
        }
        if problems.is_empty() {
            passed += 1;
        } else {
            failures.push(format!("{}: {}", case["id"], problems.join("; ")));
        }
        results.push(
            json!({"id":case["id"],"check":case["check"],"category":case["category"],
            "result":if problems.is_empty(){"pass"}else{"fail"},"problems":problems,
            "report":actual.report.to_json()}),
        );
    }
    let total = results.len() - informative;
    let name = &if kinds.is_empty() {
        name.to_owned()
    } else {
        format!("{name}[{}]", kinds.join(","))
    };
    eprintln!(
        "{name}: {passed}/{total} normative cases passed, {} failed, {informative} informative not counted",
        total - passed
    );
    if let Some(dir) = std::env::var_os("LOJ_CONFORMANCE_REPORT_DIR") {
        let path =
            Path::new(&dir).join(format!("{}.json", name.replace(['/', '[', ']', ','], "-")));
        fs::write(path, serde_json::to_vec_pretty(&results).unwrap()).unwrap();
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn package_v1alpha2() {
    suite("v1alpha2");
}

#[test]
fn history_v1alpha1() {
    suite("loj-history/v1alpha1");
}

#[test]
fn glyphs_v1alpha1() {
    suite("loj-glyphs/v1alpha1");
}

#[test]
fn registry_v1alpha1() {
    suite("loj-registry/v1alpha1");
}

#[test]
fn common_v1alpha1() {
    suite("common/v1alpha1");
}

#[test]
fn rebuild_v1alpha1() {
    suite("loj-rebuild/v1alpha1");
}
