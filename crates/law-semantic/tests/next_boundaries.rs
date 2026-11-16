//! Synthetic boundaries beyond the fixed conformance corpus.
#![allow(clippy::unwrap_used)]
use law_semantic::checks::{run_with_space, Input};
use law_semantic::common::{Check, ResultValue};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

#[test]
fn library_git_cancellation_is_incomplete_not_a_violation() {
    use std::sync::{atomic::AtomicBool, Arc};
    let scratch = Scratch::new();
    scratch.write(
        "input.bundle",
        b"synthetic bundle; cancellation precedes git",
    );
    let input = Input::Change {
        bundle: scratch.0.join("input.bundle"),
        base: "aaaa".into(),
        head: "bbbb".into(),
    };
    let report = law_semantic::cancellation::with_flag(Arc::new(AtomicBool::new(true)), || {
        run_with_space(&input, &[Check::CHANGE], None)
    });
    assert_eq!(report.result(Check::CHANGE), Some(ResultValue::Incomplete));
    assert_eq!(report.exit_code(), 3);
    // The git error is replaced, as every partial finding is, by the one
    // cancellation diagnostic.
    assert_eq!(report.diagnostics.len(), 1, "{:?}", report.diagnostics);
    assert_eq!(
        report.diagnostics[0].message,
        law_semantic::cancellation::CANCELLED
    );
    assert!(!report.diagnostics[0].is_error());
}

/// Every public next-specification entry point turns a cancelled scope into
/// `incomplete`, whatever the input would otherwise produce, and outside a
/// scope the same inputs keep their results.
#[test]
fn library_cancellation_without_git_is_incomplete_at_every_entry_point() {
    use std::sync::{atomic::AtomicBool, Arc};
    let cancelled = |run: &dyn Fn() -> law_semantic::common::Report| {
        law_semantic::cancellation::with_flag(Arc::new(AtomicBool::new(true)), run)
    };
    let only_cancellation = |report: &law_semantic::common::Report| {
        assert!(
            report
                .results
                .iter()
                .all(|(_, r)| *r == ResultValue::Incomplete),
            "{:?}",
            report.results
        );
        assert_eq!(report.exit_code(), 3);
        assert_eq!(report.diagnostics.len(), 1, "{:?}", report.diagnostics);
        assert_eq!(report.diagnostics[0].rule, "VAL-04");
        assert!(!report.diagnostics[0].is_error());
        assert!(report.references.is_empty() && report.resolution.is_none());
    };
    let dir = Scratch::new();
    // A failing package: cancellation must not keep its violation either.
    let bad = "# 第一条\n<!-- loj:unknown -->";
    let checked = package(&dir, bad);
    assert_eq!(checked.report.result(Check::L1), Some(ResultValue::Fail));
    let levels = [Check::L0, Check::L1];
    let checked = law_semantic::cancellation::with_flag(Arc::new(AtomicBool::new(true)), || {
        law_semantic::v1alpha2::check(&dir.0, &levels)
    });
    only_cancellation(&checked.report);
    assert!(checked.ast.is_none() && checked.sources.is_empty());
    for input in [
        Input::Package(dir.0.clone()),
        Input::Registry(dir.0.clone()),
        Input::Records {
            registry: dir.0.clone(),
            repositories: vec![dir.0.clone()],
        },
    ] {
        let checks = input.default_checks();
        only_cancellation(&cancelled(&|| {
            run_with_space(&input, &checks, Some("alpha"))
        }));
    }
    let good = "# 第一条\n<!-- loj:paragraph number=1 -->\n本文。\n<!-- loj:end-paragraph -->";
    assert_eq!(
        package(&dir, good).report.result(Check::L1),
        Some(ResultValue::Pass)
    );
    only_cancellation(&cancelled(&|| {
        run_with_space(&Input::Package(dir.0.clone()), &[Check::L1], None)
    }));
    // A structured reference refused before any git or file access.
    let bundle = dir.0.join("absent.bundle");
    let value = json!({"record": "not a record"});
    let refused = law_semantic::fixref::check_structured(&bundle, &dir.0, &value, None);
    assert_ne!(
        refused.result(Check::REFERENCE),
        Some(ResultValue::Incomplete)
    );
    only_cancellation(&cancelled(&|| {
        law_semantic::fixref::check_structured(&bundle, &dir.0, &value, None)
    }));
    only_cancellation(&cancelled(&|| {
        law_semantic::fixref::check(&bundle, &dir.0, "not a reference")
    }));
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "loj-next-boundaries-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn record(id: &str) -> Value {
    json!({"schemaVersion":"loj-registry/v1alpha1","id":id,"label":"Synthetic record",
        "status":"active","identity":{"evidence":[{"source":"s","locator":"1"}],"verification":{"state":"confirmed"}},
        "collection":{"state":"notCollected"},"sources":[{"id":"s","kind":"other","title":"Synthetic source"}],"numbers":[],"externalIds":[]})
}
#[test]
fn registry_space_is_explicit_and_replaced_by_requires_the_same_registry() {
    let dir = Scratch::new();
    let a = record("a7k2m9qd3x");
    let mut b = record("b7k2m9qd3x");
    b["status"] = json!("merged");
    b["replacedBy"] = json!([{"record":{"id":"a7k2m9qd3x","space":"alpha"},"evidence":[{"source":"s","locator":"2"}],"verification":{"state":"confirmed"}}]);
    dir.write("records/a7k2m9qd3x.yaml", a.to_string());
    dir.write("records/b7k2m9qd3x.yaml", b.to_string());
    let input = Input::Registry(dir.0.clone());
    let check = |space| run_with_space(&input, &[Check::REGISTRY], space);
    assert_eq!(
        check(Some("alpha")).result(Check::REGISTRY),
        Some(ResultValue::Pass)
    );
    assert_eq!(
        check(None).result(Check::REGISTRY),
        Some(ResultValue::Incomplete)
    );
    assert_eq!(
        check(Some("beta")).result(Check::REGISTRY),
        Some(ResultValue::Fail)
    );
    let before = check(Some("alpha")).to_json();
    for bytes in [b"id: beta".as_slice(), b"not: [yaml", b"\xff"] {
        dir.write("space.yaml", bytes);
        assert_eq!(check(Some("alpha")).to_json(), before);
    }
    b["replacedBy"][0]["record"]
        .as_object_mut()
        .unwrap()
        .remove("space");
    dir.write("records/b7k2m9qd3x.yaml", b.to_string());
    assert_eq!(check(None).result(Check::REGISTRY), Some(ResultValue::Pass));
}
fn package(dir: &Scratch, text: &str) -> law_semantic::v1alpha2::PackageCheck {
    dir.write("law.yaml", json!({"schemaVersion":"loj/v1alpha2","law":{"id":"synthetic","title":"Synthetic law","type":"act","number":null,"promulgatedAt":null,"language":"ja"},"documents":[{"path":"main.md","parent":"main"}]}).to_string());
    dir.write("main.md", text);
    law_semantic::v1alpha2::check(&dir.0, &[Check::L0, Check::L1])
}
#[test]
fn paragraph_caption_is_explicit_and_must_precede_children() {
    let dir = Scratch::new();
    let good = "# 第一条\n<!-- loj:paragraph number=1 caption=present -->\n（見出し）\n本文。\n<!-- loj:end-paragraph -->";
    let result = package(&dir, good);
    assert_eq!(
        result.report.result(Check::L1),
        Some(ResultValue::Pass),
        "{:?}",
        result.report.diagnostics
    );
    assert!(result.ast.unwrap().to_string().contains("caption"));
    let plain = package(&dir, &good.replace(" caption=present", ""));
    assert!(!plain.ast.unwrap().to_string().contains("caption"));
    let bad = "# 第一条\n<!-- loj:paragraph number=1 caption=present -->\n<!-- loj:item number=1 -->\n一　子の本文。\n<!-- loj:end-item -->\n（見出し）\n本文。\n<!-- loj:end-paragraph -->";
    let result = package(&dir, bad);
    assert!(result
        .report
        .diagnostics
        .iter()
        .any(|d| d.rule == "PARA-02"));
}
#[test]
fn diagnostic_offsets_use_original_crlf_and_four_byte_characters() {
    let dir = Scratch::new();
    let text = "# 第一条\r\n<!-- loj:paragraph number=1 -->\r\n😀本文。\r\n<!-- loj:unknown -->";
    let result = package(&dir, text);
    let start = text.find("<!-- loj:unknown").unwrap();
    assert!(
        result.report.diagnostics.iter().any(|d| d.rule == "DIR-01"
            && d.location
                .as_ref()
                .is_some_and(|l| l["start"] == start && l["end"] == text.len())),
        "{:?}",
        result.report.diagnostics
    );
}

#[test]
fn fragmented_caption_diagnostic_uses_the_later_source_fragment() {
    let dir = Scratch::new();
    let text = "# 第一条\r\n<!-- loj:paragraph number=1 caption=present -->\r\n\r\n<!-- loj:unknown -->\r\n(bad caption)\r\n本文。\r\n<!-- loj:end-paragraph -->";
    let result = package(&dir, text);
    let start = text.find("(bad caption)").unwrap();
    assert!(
        result.report.diagnostics.iter().any(|d| d.rule == "PARA-02"
            && d.location.as_ref().is_some_and(
                |l| l["start"] == start && l["end"] == start + "(bad caption)\r\n".len()
            )),
        "{:?}",
        result.report.diagnostics
    );
}
#[test]
fn duplicate_or_unidentified_l4_inputs_are_incomplete() {
    let dir = Scratch::new();
    dir.write(
        "registry/records/a7k2m9qd3x.yaml",
        record("a7k2m9qd3x").to_string(),
    );
    let h = json!({"schemaVersion":"loj-history/v1alpha1","record":{"id":"a7k2m9qd3x"},"revisions":[],"relations":[]});
    dir.write("a/history.yaml", h.to_string());
    dir.write("b/history.yaml", h.to_string());
    let input = Input::Records {
        registry: dir.0.join("registry"),
        repositories: vec![dir.0.join("a"), dir.0.join("b")],
    };
    let checks = input.default_checks();
    for value in [
        h,
        json!({"schemaVersion":"loj-history/v1alpha1","record":{"id":false}}),
    ] {
        dir.write("b/history.yaml", value.to_string());
        let report = run_with_space(&input, &checks, Some("alpha"));
        assert!(
            report
                .results
                .iter()
                .all(|(_, r)| *r == ResultValue::Incomplete),
            "{:?}",
            report.results
        );
    }
}
