//! Synthetic boundaries beyond the fixed conformance corpus.
#![allow(clippy::unwrap_used)]
use law_semantic::checks::{run_with_space, Input};
use law_semantic::common::{Check, Class, ResultValue};
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
    let d = &report.diagnostics[0];
    assert_eq!((d.rule.as_str(), d.class), ("VAL-04", Class::Undetermined));
    assert_eq!(d.code, "common/v1alpha1/cancelled");
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

fn rules(result: &law_semantic::v1alpha2::PackageCheck) -> Vec<String> {
    result
        .report
        .diagnostics
        .iter()
        .filter(|d| d.class == Class::Error)
        .map(|d| d.rule.clone())
        .collect()
}

#[test]
fn amendment_contents_keep_their_own_article_context() {
    let dir = Scratch::new();
    // Two amendments: the first content holds a deleted and an active quoted
    // article; the second content holds a paragraph group. Neither content's
    // article context leaks into the next content or into the amending law.
    let text = "# 第一条\n<!-- loj:paragraph number=1 -->\n甲法の一部を次のように改正する。\n\
<!-- loj:amendment number=1 -->\n第四条及び第五条を次のように改める。\n<!-- loj:amendment-content -->\n\
# 第四条\n削除\n\n# 第五条（記録）\n<!-- loj:paragraph number=1 -->\n記録する。\n<!-- loj:end-paragraph -->\n\
<!-- loj:end-amendment-content -->\n<!-- loj:end-amendment -->\n\
<!-- loj:amendment number=2 -->\n第六条に次の一項を加える。\n<!-- loj:amendment-content -->\n\
<!-- loj:paragraph number=2 -->\n２　公表する。\n<!-- loj:end-paragraph -->\n\
<!-- loj:end-amendment-content -->\n<!-- loj:end-amendment -->\n<!-- loj:end-paragraph -->\n\
# 第二条\n<!-- loj:paragraph number=1 -->\n乙法の一部を次のように改正する。\n\
<!-- loj:item number=1 -->\n一　第二条を次のように改める。\n<!-- loj:amendment number=1 -->\n第二条を次のように改める。\n\
<!-- loj:amendment-content -->\n# 第二条\n<!-- loj:paragraph number=1 -->\n雪を記録する。\n<!-- loj:end-paragraph -->\n\
<!-- loj:end-amendment-content -->\n<!-- loj:end-amendment -->\n<!-- loj:end-item -->\n<!-- loj:end-paragraph -->\n";
    let result = package(&dir, text);
    assert_eq!(
        result.report.result(Check::L1),
        Some(ResultValue::Pass),
        "{:?}",
        result.report.diagnostics
    );
    let ast = result.ast.unwrap().to_string();
    let first = "synthetic/main/article/1/paragraph/1/amendment/1/content";
    assert!(ast.contains(&format!("\"{first}/article/4\"")), "{ast}");
    assert!(ast.contains("\"status\":\"deleted\""), "{ast}");
    assert!(ast.contains(&format!("\"{first}/article/5/paragraph/1\"")));
    let second = "synthetic/main/article/1/paragraph/1/amendment/2/content/paragraph/2";
    assert!(ast.contains(&format!("\"{second}\"")), "{ast}");
    assert!(ast.contains("\"synthetic/main/article/2/paragraph/1\""));
    let quoted = "synthetic/main/article/2/paragraph/1/item/1/amendment/1/content/article/2";
    assert!(ast.contains(&format!("\"{quoted}\"")), "{ast}");
}

#[test]
fn text_and_headings_in_amendment_contents_follow_the_document_rules() {
    let dir = Scratch::new();
    let head = "# 第一条\n<!-- loj:paragraph number=1 -->\n甲法の一部を次のように改正する。\n\
<!-- loj:amendment number=1 -->\n第五条を次のように改める。\n<!-- loj:amendment-content -->\n";
    let tail = "<!-- loj:end-amendment-content -->\n<!-- loj:end-amendment -->\n<!-- loj:end-paragraph -->\n";
    // Text after a quoted article heading, outside its paragraphs: MD-03.
    let loose = format!(
        "{head}# 第五条\n本文。\n<!-- loj:paragraph number=1 -->\n記録する。\n<!-- loj:end-paragraph -->\n{tail}"
    );
    assert!(rules(&package(&dir, &loose)).contains(&"MD-03".to_owned()));
    // A supplement heading in a content is unrecognized (HEAD-01), never HEAD-03.
    let supplement = format!(
        "{head}# 附　則\n<!-- loj:paragraph number=1 -->\n施行する。\n<!-- loj:end-paragraph -->\n{tail}"
    );
    let found = rules(&package(&dir, &supplement));
    assert!(found.contains(&"HEAD-01".to_owned()), "{found:?}");
    assert!(!found.contains(&"HEAD-03".to_owned()), "{found:?}");
    // In the article form, a paragraph after an article belongs to it.
    let article_form =
        "# 第一条\n<!-- loj:paragraph number=1 -->\n本文。\n<!-- loj:end-paragraph -->\n\
<!-- loj:paragraph number=2 -->\n２　本文。\n<!-- loj:end-paragraph -->\n";
    let result = package(&dir, article_form);
    assert_eq!(
        result.report.result(Check::L1),
        Some(ResultValue::Pass),
        "{:?}",
        result.report.diagnostics
    );
    assert!(result
        .ast
        .unwrap()
        .to_string()
        .contains("\"synthetic/main/article/1/paragraph/2\""));
}

#[test]
fn amendment_numbers_and_content_counts_are_checked() {
    let dir = Scratch::new();
    let amendment = |number: &str, contents: usize| {
        let content =
            "<!-- loj:amendment-content -->\n（見出し）\n<!-- loj:end-amendment-content -->\n";
        format!(
            "<!-- loj:amendment number={number} -->\n第五条の見出しを次のように改める。\n{}<!-- loj:end-amendment -->\n",
            content.repeat(contents)
        )
    };
    let law = |amendments: &str| {
        format!(
            "# 第一条\n<!-- loj:paragraph number=1 -->\n甲法の一部を次のように改正する。\n{amendments}<!-- loj:end-paragraph -->\n"
        )
    };
    let ok = law(&(amendment("1", 1) + &amendment("2", 1)));
    let result = package(&dir, &ok);
    assert_eq!(
        result.report.result(Check::L1),
        Some(ResultValue::Pass),
        "{:?}",
        result.report.diagnostics
    );
    for (name, text) in [
        ("gap", law(&(amendment("1", 1) + &amendment("3", 1)))),
        ("duplicate", law(&(amendment("1", 1) + &amendment("1", 1)))),
        ("no content", law(&amendment("1", 0))),
        ("two contents", law(&amendment("1", 2))),
    ] {
        let found = rules(&package(&dir, &text));
        assert!(found.contains(&"AMEND-01".to_owned()), "{name}: {found:?}");
    }
}

#[test]
fn amendment_contents_accept_any_subitem_level_and_must_be_closed() {
    let dir = Scratch::new();
    let head = "# 第一条\n<!-- loj:paragraph number=1 -->\n甲法の一部を次のように改正する。\n\
<!-- loj:amendment number=1 -->\n第五条第一号イに次のように加える。\n<!-- loj:amendment-content -->\n";
    let body = "<!-- loj:subitem level=2 number=1 -->\n（１）　霧\n<!-- loj:end-subitem -->\n";
    let tail = "<!-- loj:end-amendment-content -->\n<!-- loj:end-amendment -->\n<!-- loj:end-paragraph -->\n";
    let result = package(&dir, &format!("{head}{body}{tail}"));
    assert_eq!(
        result.report.result(Check::L1),
        Some(ResultValue::Pass),
        "{:?}",
        result.report.diagnostics
    );
    // A content left open at the end of the file fails; it never passes silently.
    let unclosed = package(&dir, &format!("{head}{body}"));
    assert_eq!(unclosed.report.result(Check::L1), Some(ResultValue::Fail));
    assert!(rules(&unclosed).contains(&"DIR-02".to_owned()));
}

#[test]
fn deeply_nested_amendments_are_incomplete_not_a_crash() {
    let dir = Scratch::new();
    let open = "<!-- loj:amendment number=1 -->\n改める。\n<!-- loj:amendment-content -->\n\
<!-- loj:paragraph number=1 -->\n本文。\n";
    let close = "<!-- loj:end-paragraph -->\n<!-- loj:end-amendment-content -->\n<!-- loj:end-amendment -->\n";
    let depth = 5_000;
    let text = format!(
        "# 第一条\n<!-- loj:paragraph number=1 -->\n本文。\n{}{}<!-- loj:end-paragraph -->\n",
        open.repeat(depth),
        close.repeat(depth)
    );
    let result = package(&dir, &text);
    assert_eq!(
        result.report.result(Check::L1),
        Some(ResultValue::Incomplete)
    );
}

/// LIST-01/02 boundaries beyond pkg-208..216.
#[test]
fn lists_are_numbered_in_order_and_kept_apart_from_items() {
    let dir = Scratch::new();
    let list = |n: &str, text: &str| {
        format!("<!-- loj:list number={n} -->\n{text}\n<!-- loj:end-list -->\n")
    };
    let paragraph = |body: &str| {
        format!("# 第一条\n<!-- loj:paragraph number=1 -->\n左の局を置く。\n{body}<!-- loj:end-paragraph -->\n")
    };
    // Lists beside a table in one parent; the order counts lists only, and
    // the text after the last list resumes the parent.
    let table = "<!-- loj:table number=1 -->\n<!-- loj:row key=k3v9 -->\n<!-- loj:cell -->\n欄\n<!-- loj:end-cell -->\n<!-- loj:end-row -->\n<!-- loj:end-table -->\n";
    let text = paragraph(&format!(
        "{}{table}{}続き。\n",
        list("1", "一　雲局"),
        list("2", "雨局")
    ));
    let result = package(&dir, &text);
    assert_eq!(
        rules(&result),
        Vec::<String>::new(),
        "{:?}",
        result.report.diagnostics
    );
    let ast = result.ast.unwrap();
    let p = &ast["root"]["content"][0]["content"][0]["content"][0];
    let first = &p["content"][1];
    assert_eq!(first["kind"], "List");
    assert_eq!(first["id"], "synthetic/main/article/1/paragraph/1/list/1");
    assert_eq!(first["display"], "");
    assert_eq!(first["number"], "1");
    // The leading 一 is text, not a display number.
    assert_eq!(first["content"], json!([{"text": "一　雲局\n"}]));
    assert_eq!(
        p["content"].as_array().unwrap().last().unwrap(),
        &json!({"text": "続き。\n"})
    );

    // A branch number, a gap and a start other than 1 break the order.
    for numbers in [["1", "1-2"], ["1", "3"], ["2", "3"]] {
        let text = paragraph(&format!(
            "{}{}",
            list(numbers[0], "雲局"),
            list(numbers[1], "雨局")
        ));
        let found = rules(&package(&dir, &text));
        assert!(
            !found.is_empty() && found.iter().all(|r| r == "LIST-01"),
            "{numbers:?}: {found:?}"
        );
    }
    // Items and lists in one paragraph are LIST-02.
    let item = "<!-- loj:item number=1 -->\n一　雲\n<!-- loj:end-item -->\n";
    let text = paragraph(&format!("{item}{}", list("1", "雨局")));
    assert_eq!(rules(&package(&dir, &text)), ["LIST-02"]);
    // A list directly under an article is a parent error (in a list: pkg-213).
    let text = format!("# 第一条\n{}", list("1", "雲局"));
    assert!(rules(&package(&dir, &text)).contains(&"DIR-02".to_owned()));
    // A heading inside a list leaves it unclosed.
    let text = paragraph("<!-- loj:list number=1 -->\n雲局\n# 第二条\n");
    let found = rules(&package(&dir, &text));
    assert!(found.contains(&"MD-03".to_owned()), "{found:?}");
}

/// AMEND-02: lists are a group of an amendment content of their own.
#[test]
fn an_amendment_content_may_be_a_group_of_lists() {
    let dir = Scratch::new();
    let content = |body: &str| {
        format!(
            "# 第一条\n<!-- loj:paragraph number=1 -->\n甲法の一部を次のように改正する。\n\
<!-- loj:amendment number=1 -->\n第三条に次の局を加える。\n<!-- loj:amendment-content -->\n{body}\
<!-- loj:end-amendment-content -->\n<!-- loj:end-amendment -->\n<!-- loj:end-paragraph -->\n"
        )
    };
    let lists = "<!-- loj:list number=1 -->\n雪局\n<!-- loj:end-list -->\n<!-- loj:list number=2 -->\n霧局\n<!-- loj:end-list -->\n";
    let result = package(&dir, &content(lists));
    assert_eq!(
        rules(&result),
        Vec::<String>::new(),
        "{:?}",
        result.report.diagnostics
    );
    assert!(result
        .ast
        .unwrap()
        .to_string()
        .contains("\"synthetic/main/article/1/paragraph/1/amendment/1/content/list/2\""));
    let table = "<!-- loj:table number=1 -->\n<!-- loj:row key=k3v9 -->\n<!-- loj:cell -->\n欄\n<!-- loj:end-cell -->\n<!-- loj:end-row -->\n<!-- loj:end-table -->\n";
    assert_eq!(
        rules(&package(&dir, &content(&format!("{lists}{table}")))),
        ["AMEND-02"]
    );
}

/// AMEND-01 `instruction=implicit` boundaries beyond pkg-217..222.
#[test]
fn amendments_without_instruction_text_hold_one_content_only() {
    let dir = Scratch::new();
    let implicit = |n: usize, article: &str| {
        format!("<!-- loj:amendment number={n} instruction=implicit -->\n<!-- loj:amendment-content -->\n# {article}\n削除\n\n<!-- loj:end-amendment-content -->\n<!-- loj:end-amendment -->\n")
    };
    let explicit = "<!-- loj:amendment number=1 -->\n第六條を次のように改める。\n<!-- loj:amendment-content -->\n# 第六條\n削除\n\n<!-- loj:end-amendment-content -->\n<!-- loj:end-amendment -->\n";
    let paragraph = |body: &str| {
        format!("# 第一条\n<!-- loj:paragraph number=1 -->\n甲法の一部を次のように改正する。\n{body}<!-- loj:end-paragraph -->\n")
    };
    // Instruction text between two implicit amendments separates them; an
    // implicit amendment may follow one with instruction text directly.
    let text = paragraph(&format!(
        "{}第八條を削る。\n{}",
        implicit(1, "第七條"),
        implicit(2, "第九條")
    ));
    let result = package(&dir, &text);
    assert_eq!(
        rules(&result),
        Vec::<String>::new(),
        "{:?}",
        result.report.diagnostics
    );
    let ast = result.ast.unwrap().to_string();
    assert!(ast.contains("\"instruction\":\"implicit\""), "{ast}");
    let text = paragraph(&format!("{explicit}{}", implicit(2, "第七條")));
    let result = package(&dir, &text);
    assert_eq!(
        rules(&result),
        Vec::<String>::new(),
        "{:?}",
        result.report.diagnostics
    );
    let ast = result.ast.unwrap().to_string();
    assert!(ast.contains("\"instruction\":\"explicit\""), "{ast}");

    // Only blank text between two implicit amendments: the second fails.
    let text = paragraph(&format!(
        "{}\n{}",
        implicit(1, "第七條"),
        implicit(2, "第九條")
    ));
    assert_eq!(rules(&package(&dir, &text)), ["AMEND-01"]);
    // A blank line before the content, and text after it, are AMEND-01.
    let blank_before = implicit(1, "第七條").replacen(
        " -->\n<!-- loj:amendment-content",
        " -->\n\n<!-- loj:amendment-content",
        1,
    );
    assert_eq!(
        rules(&package(&dir, &paragraph(&blank_before))),
        ["AMEND-01"]
    );
    let text_after = implicit(1, "第七條").replace(
        "<!-- loj:end-amendment -->",
        "附記\n<!-- loj:end-amendment -->",
    );
    assert_eq!(rules(&package(&dir, &paragraph(&text_after))), ["AMEND-01"]);
    // An amendment with instruction text keeps requiring it.
    let no_instruction = explicit.replace("第六條を次のように改める。\n", "");
    assert_eq!(
        rules(&package(&dir, &paragraph(&no_instruction))),
        ["AMEND-01"]
    );
}

/// HEAD-04: `削除` is followed by a blank line or the end of the file, at the
/// document level as in an amendment content.
#[test]
fn a_deleted_article_needs_a_blank_line_or_the_end_of_the_file() {
    let dir = Scratch::new();
    let active = "# 第二条\n<!-- loj:paragraph number=1 -->\n本文。\n<!-- loj:end-paragraph -->\n";
    for (text, expected) in [
        (format!("# 第一条\n削除\n\n{active}"), Vec::<&str>::new()),
        ("# 第一条\n削除\n".to_owned(), vec![]),
        ("# 第一条\n削除".to_owned(), vec![]),
        (format!("# 第一条\n削除\n{active}"), vec!["HEAD-04"]),
    ] {
        let result = package(&dir, &text);
        assert_eq!(
            rules(&result),
            expected,
            "{text:?}: {:?}",
            result.report.diagnostics
        );
        if let Some(ast) = result.ast {
            assert!(
                ast.to_string().contains("\"status\":\"deleted\""),
                "{text:?}"
            );
        }
    }
}
