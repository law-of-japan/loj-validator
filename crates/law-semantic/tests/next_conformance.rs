//! Formal conformance of the next specification (vendored loj-spec, see
//! vendor/loj-spec/COMMIT): one test per suite. Expected results are never
//! adjusted to the implementation (CONF-01).
// Test helpers outside `#[test]` functions may unwrap, and offsets read
// back from JSON are cast for slicing: a failure here fails the test.
#![allow(clippy::unwrap_used, clippy::cast_possible_truncation)]
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
            report: law_semantic::checks::run_with_space(
                &law_semantic::checks::Input::Repository(
                    root.join(input["path"].as_str().unwrap()),
                ),
                checks,
                Some("loj"),
            ),
            ast: None,
            positions: Vec::new(),
        },
        "registry" => Actual {
            report: law_semantic::checks::run_with_space(
                &law_semantic::checks::Input::Registry(root.join(input["path"].as_str().unwrap())),
                checks,
                Some("loj"),
            ),
            ast: None,
            positions: Vec::new(),
        },
        "records" => Actual {
            report: law_semantic::checks::run_with_space(
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
                Some("loj"),
            ),
            ast: None,
            positions: Vec::new(),
        },
        "change" => Actual {
            report: law_semantic::checks::run_with_space(
                &law_semantic::checks::Input::Change {
                    bundle: root.join(input["bundle"].as_str().unwrap()),
                    // `base` is omitted for a check from the first commit.
                    base: input["base"].as_str().map(Into::into),
                    head: input["head"].as_str().unwrap().into(),
                },
                checks,
                Some("loj"),
            ),
            ast: None,
            positions: Vec::new(),
        },
        "reference" => Actual {
            report: law_semantic::checks::run_with_space(
                &law_semantic::checks::Input::Reference {
                    bundle: root.join(input["bundle"].as_str().unwrap()),
                    registry: root.join(input["registry"].as_str().unwrap()),
                    reference: input["reference"].as_str().unwrap().into(),
                },
                checks,
                Some("loj"),
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
        "incomplete" => ResultValue::Incomplete,
        _ => panic!("unknown expected result: {s}"),
    }
}

/// CONF-01 compares diagnostic identity and only explicitly expected positions.
fn diagnostic_matches(expected: &Value, actual: &Value) -> bool {
    ["rule", "schemaVersion", "class"].iter().all(|key| {
        expected
            .get(*key)
            .is_some_and(|value| value.is_string() && actual.get(*key) == Some(value))
    }) && expected.get("location").is_none_or(|location| {
        ["path", "start", "end"].iter().all(|key| {
            location.get(*key).is_some_and(|value| {
                let valid = if *key == "path" {
                    value.is_string()
                } else {
                    value.is_u64()
                };
                valid && actual["location"].get(*key) == Some(value)
            })
        })
    })
}

/// VAL-07 assigns common types to their containing file's check. The report
/// does not carry a check on diagnostics, so use its rule and source location.
fn diagnostic_belongs_to(d: &Value, own: &[Check]) -> bool {
    let rule = d["rule"].as_str().unwrap_or("");
    let path = d["location"]["path"]
        .as_str()
        .or_else(|| d["location"]["file"].as_str())
        .unwrap_or("")
        .replace('\\', "/");
    let family = rule.split('-').next().unwrap_or("");
    let check = match family {
        "XREF" => match rule {
            "XREF-01" => Check::RELATION_RESOLUTION,
            "XREF-02" => Check::RELATION_CONSISTENCY,
            "XREF-03" | "XREF-06" => Check::TIME_CONSISTENCY,
            "XREF-05" => Check::DISPLAY_CONSISTENCY,
            _ => panic!("unclassified cross-record diagnostic: {rule}"),
        },
        "BASIC" | "MAN" => match rule {
            "BASIC-03" => Check::L1,
            "BASIC-04" | "MAN-03" if path.ends_with(".md") => Check::L1,
            _ => Check::L0,
        },
        "MD" | "HEAD" | "NUM" | "DIR" | "DISP" | "AST" | "SRC" | "ID" | "REF" | "PARA"
        | "TABLE" | "PRE" | "TOC" => Check::L1,
        "HIST" | "REV" => Check::HISTORY_STRUCTURE,
        "TYPE" | "TIME" | "GLYPH" | "FIXREF" | "RID" => {
            if path.contains("records/") {
                Check::REGISTRY
            } else if path.ends_with("rebuild.yaml") {
                Check::REBUILD
            } else if family == "RID" {
                Check::REGISTRY
            } else if family == "FIXREF"
                && !path.ends_with("history.yaml")
                && !path.ends_with("glyphs.yaml")
            {
                Check::REFERENCE
            } else if family == "TIME" {
                Check::HISTORY_STRUCTURE
            } else {
                Check::EVIDENCE
            }
        }
        "REG" => Check::REGISTRY,
        "REB" | "REBR" => Check::REBUILD,
        "CHG" => Check::CHANGE,
        _ => panic!("unclassified diagnostic family: {rule}"),
    };
    own.contains(&check)
}

#[test]
#[should_panic(expected = "unclassified diagnostic family")]
fn unknown_rule_families_cannot_hide_errors() {
    diagnostic_belongs_to(&json!({"rule":"FUTURE-01"}), &[Check::L1]);
}

#[test]
fn expected_diagnostic_classes_are_required() {
    use law_semantic::common::{Class, Diagnostic};

    let case = json!({"check": "L1", "success": true});
    for class in [Class::Warning, Class::Undetermined, Class::Pending] {
        let diagnostic = Diagnostic::error("MD-01", "loj/v1alpha2", "synthetic").with_class(class);
        let expected = json!({"diagnostics": [diagnostic.to_json()]});
        let mut actual = Actual {
            report: Report::default(),
            ast: None,
            positions: Vec::new(),
        };
        actual.report.set(Check::L1, ResultValue::Pass);
        assert!(!problems(&case, &expected, &actual).is_empty());
        actual.report.diagnostics.push(diagnostic);
        assert!(problems(&case, &expected, &actual).is_empty());
        actual.report.diagnostics[0].class = Class::Error;
        assert!(!diagnostic_matches(
            &expected["diagnostics"][0],
            &actual.report.diagnostics[0].to_json()
        ));
    }
}

#[test]
fn cases_compare_only_their_expected_results_and_own_errors() {
    use law_semantic::common::Diagnostic;
    let mut actual = Actual {
        report: Report::default(),
        ast: None,
        positions: Vec::new(),
    };
    actual.report.set(Check::L0, ResultValue::Pass);
    let case = json!({"check": "L0", "success": true});
    let expected = json!({"results": [{"check": "L0", "result": "pass"}]});
    assert!(problems(&case, &expected, &actual).is_empty());
    actual
        .report
        .diagnostics
        .push(Diagnostic::error("HEAD-01", "loj/v1alpha2", "outside L0"));
    actual.report.set(Check::L1, ResultValue::Fail);
    assert!(problems(&case, &expected, &actual).is_empty());
    actual
        .report
        .diagnostics
        .push(Diagnostic::error("MAN-01", "loj/v1alpha2", "inside L0"));
    assert!(!problems(&case, &expected, &actual).is_empty());

    actual.report.diagnostics.clear();
    actual
        .report
        .set(Check::RELATION_RESOLUTION, ResultValue::Incomplete);
    let case = json!({"check": "L4", "category": "relationResolution", "success": true});
    let expected = json!({"results": [{"check": "L4", "category": "relationResolution", "result": "incomplete"}]});
    actual.report.diagnostics.push(Diagnostic::error(
        "HIST-04",
        "loj-history/v1alpha1",
        "outside L4",
    ));
    assert!(problems(&case, &expected, &actual).is_empty());
    actual.report.diagnostics.push(Diagnostic::error(
        "XREF-02",
        "common/v1alpha1",
        "other L4 category",
    ));
    assert!(problems(&case, &expected, &actual).is_empty());
    actual.report.diagnostics.push(Diagnostic::error(
        "XREF-01",
        "common/v1alpha1",
        "own category",
    ));
    assert!(!problems(&case, &expected, &actual).is_empty());
}

#[test]
fn expected_diagnostic_locations_are_required() {
    let expected = json!({
        "rule": "MAN-03", "schemaVersion": "loj/v1alpha2", "class": "error",
        "location": {"path": "second.md", "start": 0, "end": 42}
    });
    let mut actual = expected.clone();
    actual["message"] = json!("implementation-specific explanation");
    actual["location"]["line"] = json!(1);
    assert!(diagnostic_matches(&expected, &actual));
    for (key, wrong) in [
        ("path", json!("first.md")),
        ("start", json!(1)),
        ("end", json!(41)),
    ] {
        let mut mismatch = actual.clone();
        mismatch["location"][key] = wrong;
        assert!(!diagnostic_matches(&expected, &mismatch));
    }
    actual.as_object_mut().unwrap().remove("location");
    assert!(!diagnostic_matches(&expected, &actual));
    let mut without_location = expected.clone();
    without_location.as_object_mut().unwrap().remove("location");
    assert!(diagnostic_matches(&without_location, &expected));
    for key in ["rule", "schemaVersion", "class"] {
        let mut mismatch = expected.clone();
        mismatch[key] = json!("different");
        assert!(!diagnostic_matches(&expected, &mismatch));
    }
    for key in ["path", "start", "end"] {
        let mut malformed = expected.clone();
        malformed["location"].as_object_mut().unwrap().remove(key);
        assert!(!diagnostic_matches(&malformed, &malformed));
        malformed["location"][key] = Value::Null;
        assert!(!diagnostic_matches(&malformed, &malformed));
    }
}

fn problems(case: &Value, expected: &Value, actual: &Actual) -> Vec<String> {
    let mut problems = Vec::new();
    let report = &actual.report;
    let json = report.to_json();
    let diagnostics = json["diagnostics"].as_array().unwrap();
    if let Some(results) = expected["results"].as_array() {
        for r in results {
            let label = match r["category"].as_str() {
                Some(c) => format!("{}/{c}", r["check"].as_str().unwrap()),
                None => r["check"].as_str().unwrap().to_owned(),
            };
            let check = Check::parse(&label).unwrap()[0];
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
    // CONF-01 compares only listed results: success explicitly permits incomplete.
    if case["success"] == true {
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d["class"] == "error" && diagnostic_belongs_to(d, &own))
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
    }
    if let Some(ds) = expected["diagnostics"].as_array() {
        for d in ds {
            if !diagnostics.iter().any(|got| diagnostic_matches(d, got)) {
                problems.push(format!(
                    "missing diagnostic {d}; got {}",
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
