// Test helpers outside `#[test]` functions may unwrap, and offsets read
// back from JSON are cast for slicing: a failure here fails the test.
#![allow(clippy::unwrap_used, clippy::cast_possible_truncation)]
use law_semantic::v1alpha1::{check, Level, MAX_FILE_BYTES, MAX_TOTAL_BYTES};
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

fn fixture() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "v1-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}
fn manifest(documents: Value) -> Value {
    json!({"schemaVersion":"loj/v1alpha1","law":{"id":"20990101-act-1","title":"架空法","type":"act","number":null,"promulgatedAt":null,"language":"ja"},"documents":documents})
}
fn write_manifest(path: &std::path::Path, m: &Value) {
    fs::write(path.join("law.yaml"), serde_json::to_vec(m).unwrap()).unwrap();
}
fn run(source: impl AsRef<[u8]>, level: Level) -> Value {
    let path = fixture();
    write_manifest(
        &path,
        &manifest(json!([{"path":"main.md","parent":"main"}])),
    );
    fs::write(path.join("main.md"), source).unwrap();
    check(&path, level)
}
fn valid(body: &str) -> String {
    format!("# 第一条\n<!-- loj:paragraph number=1 -->\n{body}\n<!-- loj:end-paragraph -->\n")
}
fn rule(v: &Value, r: &str) -> bool {
    v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["rule"] == r)
}

#[test]
fn l0_accepts_structure_independently_and_unsupported_features() {
    assert_eq!(run("unstructured Markdown", Level::L0)["status"], "Passed");
    let path = fixture();
    let mut m = manifest(json!([{"path":"main.md","parent":"main"}]));
    m["unsupportedFeatures"] = json!(["preamble"]);
    write_manifest(&path, &m);
    fs::write(path.join("main.md"), "前文").unwrap();
    assert_eq!(check(&path, Level::L0)["status"], "Passed");
    assert!(rule(&check(&path, Level::L1), "BASIC-03"));
}
#[test]
fn utf8_crlf_diagnostic_and_lossless_text_positions() {
    let source="# 第一条\r\n<!-- loj:paragraph number=2 -->\r\n３　本文😀。\r\n<!-- loj:end-paragraph -->\r\n";
    let report = run(source, Level::L1);
    let d = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["rule"] == "DISP-01")
        .unwrap();
    let start = source.find('３').unwrap();
    let end = start + "３　本文😀。\r\n".len();
    assert_eq!(d["span"], json!({"start":start,"end":end}));
    let good = source.replace('３', "２");
    let r = run(&good, Level::L1);
    assert_eq!(r["status"], "Passed", "{r}");
    assert_eq!(r["sources"][1]["text"], good);
    for p in r["positions"].as_array().unwrap() {
        if let Some(spans) = p["spans"].as_array() {
            let joined: String = spans
                .iter()
                .map(|s| {
                    &good
                        [s["start"].as_u64().unwrap() as usize..s["end"].as_u64().unwrap() as usize]
                })
                .collect();
            assert_eq!(p["text"], joined);
        }
    }
}

#[test]
fn recovery_positions_and_multibyte_eof_do_not_panic() {
    let opening = "<!-- loj:paragraph number=1 -->\r\n";
    let source = format!("# 第一条\r\n{opening}本文😀");
    let r = run(&source, Level::L1);
    let d = r["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["rule"] == "DIR-02")
        .unwrap();
    let start = source.find("<!--").unwrap();
    assert_eq!(d["span"], json!({"start":start,"end":start+opening.len()}));
    let closing = "<!-- loj:end-item -->\r\n";
    let source = format!("# 第一条\r\n{opening}本文😀\r\n{closing}");
    let r = run(&source, Level::L1);
    let d = &r["diagnostics"][0];
    let start = source.find(closing).unwrap();
    assert_eq!(d["span"], json!({"start":start,"end":start+closing.len()}));
    for s in [
        "文",
        "😀",
        "# 一",
        "<!-- loj:😀",
        "# 第一条\n😀",
        "<!--\n loj:paragraph number=1",
        "本文\n\n> 文😀",
    ] {
        let r = run(s, Level::L1);
        assert_ne!(r["status"], "Passed");
    }
}
#[test]
fn invalid_unicode_bom_nul_and_cr_are_rejected() {
    for bytes in [
        vec![0xff],
        b"\xef\xbb\xbftext".to_vec(),
        b"a\0b".to_vec(),
        b"a\rb".to_vec(),
    ] {
        assert!(rule(&run(bytes, Level::L0), "BASIC-02"));
    }
}
#[test]
fn yaml_keys_tags_aliases_documents_and_scalars() {
    let path = fixture();
    fs::write(path.join("main.md"), valid("本文")).unwrap();
    let base="schemaVersion: loj/v1alpha1\nlaw:\n  id: 20990101-act-1\n  title: 架空法\n  type: act\n  number: null\n  promulgatedAt: null\n  language: ja\ndocuments:\n  - path: main.md\n    parent: main\n";
    fs::write(path.join("law.yaml"), base).unwrap();
    assert_eq!(check(&path, Level::L1)["status"], "Passed");
    for bad in [
        base.replace("title: 架空法", "title: A\n  title: B"),
        base.replace("title: 架空法", "title: &x A"),
        base.replace("title: 架空法", "title: !!str A"),
        format!("{base}---\nother: document\n"),
        base.replace("promulgatedAt: null", "promulgatedAt: 2026-11-06"),
        base.replace("title: 架空法", "title: 0x10"),
        base.replace("title: 架空法", "title: 1e999"),
    ] {
        fs::write(path.join("law.yaml"), bad).unwrap();
        assert!(rule(&check(&path, Level::L0), "MAN-01"));
    }
}
#[test]
fn paths_are_portable_and_scope_order_is_checked() {
    let path = fixture();
    fs::write(path.join("main.md"), valid("本文")).unwrap();
    for name in [
        "../main.md",
        "a/../main.md",
        "a//main.md",
        "CON.md",
        "com1.md",
        "a./main.md",
        "C:/main.md",
        "a\\main.md",
    ] {
        write_manifest(&path, &manifest(json!([{"path":name,"parent":"main"}])));
        let r = check(&path, Level::L0);
        assert_ne!(r["status"], "Passed", "{name}");
    }
    write_manifest(
        &path,
        &manifest(json!([{"path":"main.md","parent":"main"},{"path":"MAIN.md","parent":"main"}])),
    );
    assert!(rule(&check(&path, Level::L0), "MAN-02"));
    let mut m = manifest(json!([{"path":"main.md","parent":"main"}]));
    m["supplements"] = json!([{"key":"original","title":"制定附則"}]);
    write_manifest(&path, &m);
    assert!(rule(&check(&path, Level::L0), "MAN-03"));
}
#[test]
fn split_files_preserve_ast_and_ids_but_not_article_state() {
    let path = fixture();
    let first = format!("# 第一章　総則\n{}", valid("本文😀"));
    let second = valid("次条").replace("第一条", "第二条");
    fs::write(path.join("all.md"), format!("{first}{second}")).unwrap();
    write_manifest(&path, &manifest(json!([{"path":"all.md","parent":"main"}])));
    let whole = check(&path, Level::L1);
    assert_eq!(whole["status"], "Passed");
    fs::write(path.join("one.md"), &first).unwrap();
    fs::write(path.join("two.md"), &second).unwrap();
    write_manifest(
        &path,
        &manifest(json!([{"path":"one.md","parent":"main"},{"path":"two.md","parent":"main"}])),
    );
    let split = check(&path, Level::L1);
    assert_eq!(whole["ast"], split["ast"]);
    fs::write(
        path.join("two.md"),
        "<!-- loj:paragraph number=2 -->\n２　続き\n<!-- loj:end-paragraph -->\n",
    )
    .unwrap();
    assert!(rule(&check(&path, Level::L1), "DIR-02"));
}
#[test]
fn appendix_split_uses_explicit_nonfinal_boundaries() {
    let path = fixture();
    fs::write(path.join("main.md"), valid("本文")).unwrap();
    fs::write(path.join("one.md"), "# 別表\n表本文\n\n").unwrap();
    fs::write(path.join("two.md"), "次の本文\n\n## 任意の見出し\n末尾\n").unwrap();
    let mut m = manifest(
        json!([{"path":"main.md","parent":"main"},{"path":"one.md","parent":"appendix/table"},{"path":"two.md","parent":"appendix/table"}]),
    );
    m["appendices"] = json!([{"key":"table","title":"別表"}]);
    write_manifest(&path, &m);
    let r = check(&path, Level::L1);
    assert_eq!(r["status"], "Passed", "{r}");
    assert_eq!(
        r["ast"]["root"]["content"][1]["content"][0]["text"],
        "表本文\n\n次の本文\n\n## 任意の見出し\n末尾\n"
    );
    assert_eq!(check(&path, Level::L0)["status"], "Passed");
    for (first, expected) in [
        ("# 別表\n```\ncode\n", "Failed"),
        ("# 別表\n```\ncode\n```\n", "Passed"),
    ] {
        fs::write(path.join("one.md"), first).unwrap();
        let r = check(&path, Level::L1);
        assert_eq!(r["status"], expected, "{r}");
        if expected == "Failed" {
            let d = r["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .find(|d| d["rule"] == "MAN-03")
                .unwrap();
            assert_eq!(d["file"], "one.md");
            assert_eq!(d["span"], json!({"start":9,"end":13}));
        }
        assert_eq!(check(&path, Level::L0)["status"], "Passed");
    }
    // A single-file appendix still retains arbitrary headings and raw Markdown.
    m["documents"].as_array_mut().unwrap().pop();
    fs::write(
        path.join("one.md"),
        "# 別表\n本文\n\n## 任意の見出し\n末尾\n",
    )
    .unwrap();
    write_manifest(&path, &m);
    let r = check(&path, Level::L1);
    assert_eq!(r["status"], "Passed", "{r}");
    assert_eq!(
        r["ast"]["root"]["content"][1]["content"][0]["text"],
        "本文\n\n## 任意の見出し\n末尾\n"
    );
}

#[test]
fn appendix_split_keeps_violations_and_io_errors_visible() {
    let path = fixture();
    let m = {
        let mut m = manifest(json!([
            {"path":"main.md","parent":"main"},
            {"path":"one.md","parent":"appendix/table"},
            {"path":"two.md","parent":"appendix/table"}
        ]));
        m["appendices"] = json!([{"key":"table","title":"別表"}]);
        m
    };
    write_manifest(&path, &m);
    fs::write(path.join("main.md"), valid("本文")).unwrap();
    fs::write(path.join("one.md"), "# 別表\n本文\n").unwrap();
    fs::write(path.join("two.md"), "<!-- loj:item number=1 -->\n").unwrap();
    let r = check(&path, Level::L1);
    assert_eq!(r["status"], "Failed");
    assert_eq!(r["complete"], true);
    assert!(r["ast"].is_null());
    assert!(rule(&r, "HEAD-03"), "known violation must not disappear");
    assert_eq!(check(&path, Level::L0)["status"], "Passed");
    let mut missing = m;
    missing["documents"]
        .as_array_mut()
        .unwrap()
        .push(json!({"path":"missing.md","parent":"appendix/table"}));
    write_manifest(&path, &missing);
    let r = check(&path, Level::L1);
    assert_eq!(r["status"], "Error");
    assert_eq!(r["exit_code"], 2);
    assert_eq!(r["complete"], false);
    assert!(rule(&r, "MAN-02"));
    assert!(rule(&r, "HEAD-03"));
    assert_eq!(check(&path, Level::L0)["status"], "Error");
}

#[test]
fn heading_reference_definitions_do_not_cross_files() {
    let path = fixture();
    fs::write(
        path.join("one.md"),
        valid("本文\n\n[B]: https://example.com\n"),
    )
    .unwrap();
    fs::write(
        path.join("two.md"),
        valid("次条").replace("第一条", "第二条（A[B]）"),
    )
    .unwrap();
    write_manifest(
        &path,
        &manifest(json!([
            {"path":"one.md","parent":"main"}, {"path":"two.md","parent":"main"}
        ])),
    );
    let r = check(&path, Level::L1);
    assert_eq!(r["status"], "Passed", "{r}");
    assert_eq!(
        r["ast"]["root"]["content"][0]["content"][1]["display"],
        "第二条（A[B]）"
    );
}

#[test]
fn heading_punctuation_is_text_unless_commonmark_parses_markup() {
    for title in ["A#", "A[B]", "A*B"] {
        let r = run(format!("# 第一章 {title}\n{}", valid("本文")), Level::L1);
        assert_eq!(r["status"], "Passed", "{title}: {r}");
    }
    for caption in ["A[B]", "A*B", "A<B", "A`B", "A#"] {
        let source = valid("本文").replacen("第一条", &format!("第一条（{caption}）"), 1);
        let r = run(source, Level::L1);
        assert_eq!(r["status"], "Passed", "{caption}: {r}");
    }
    for caption in ["*強調*", "`code`", "<b>文字</b>"] {
        let source = valid("本文").replacen("第一条", &format!("第一条（{caption}）"), 1);
        assert!(rule(&run(source, Level::L1), "HEAD-01"));
    }
    // Reference links are resolved using the entire file, not the heading alone.
    let source =
        valid("本文\n\n[B]: https://example.com\n").replacen("第一条", "第一条（A[B]）", 1);
    assert!(rule(&run(source, Level::L1), "HEAD-01"));
}

#[test]
fn caption_trailing_whitespace_is_rejected() {
    for suffix in [" ", "\t", "　", "\u{a0}"] {
        let source = valid("本文").replacen("第一条", &format!("第一条（見出し{suffix}）"), 1);
        let r = run(source, Level::L1);
        assert_eq!(r["status"], "Failed");
        assert!(rule(&r, "HEAD-01"));
    }
    assert_eq!(
        run(
            valid("本文").replacen("第一条", "第一条（内部 空白）", 1),
            Level::L1
        )["status"],
        "Passed"
    );
}

#[test]
fn subsequent_main_and_supplement_files_require_structural_start() {
    let path = fixture();
    fs::write(path.join("main.md"), valid("本文")).unwrap();
    for supplementary in [false, true] {
        let mut m =
            manifest(json!([{"path":"main.md","parent":"main"},{"path":"two.md","parent":"main"}]));
        if supplementary {
            fs::write(
                path.join("supp.md"),
                format!("# 附則\n{}", valid("附則本文")),
            )
            .unwrap();
            m["supplements"] = json!([{"key":"original","title":"附則"}]);
            m["documents"] = json!([{"path":"main.md","parent":"main"},{"path":"supp.md","parent":"supplement/original"},{"path":"two.md","parent":"supplement/original"}]);
        }
        write_manifest(&path, &m);
        for bad in [
            "\n \n".to_owned(),
            format!(
                "<!-- ordinary -->\n{}",
                valid("次条").replace("第一条", "第二条")
            ),
            "<!-- loj:paragraph number=2 -->\n２　続き\n<!-- loj:end-paragraph -->\n".to_owned(),
        ] {
            fs::write(path.join("two.md"), bad).unwrap();
            let r = check(&path, Level::L1);
            assert_eq!(r["status"], "Failed");
            assert!(rule(&r, "MAN-03"));
        }
        for prefix in ["", "\n\n", "# 第二章 次章\n"] {
            fs::write(
                path.join("two.md"),
                format!("{prefix}{}", valid("次条").replace("第一条", "第二条")),
            )
            .unwrap();
            assert_eq!(check(&path, Level::L1)["status"], "Passed");
        }
    }
}
#[test]
fn commonmark_regions_and_strict_heading_and_markers() {
    for body in [
        "本文\n\n```\n<!-- loj:item number=1 -->\n```",
        "本文\n\n    <!-- loj:item number=1 -->",
        "本文\n\n> <!-- loj:item number=1 -->",
        "本文\n\n<div>\n<!-- loj:item number=1 -->\n</div>\n",
        "本文 `<!-- loj:item number=1 -->`",
        "本文\n\n<!-- ordinary loj:item -->",
    ] {
        let r = run(valid(body), Level::L1);
        assert_eq!(r["status"], "Passed", "{r}");
    }
    for heading in [
        " # 第一条",
        "# 第一条 #",
        "# 第一条（_強調_）",
        "第一条\n===",
        "# 第一条　",
        "# 第一条（\\*）",
    ] {
        assert!(
            rule(
                &run(valid("本文").replacen("# 第一条", heading, 1), Level::L1),
                "HEAD-01"
            ),
            "{heading}"
        );
    }
    for marker in [
        "<!-- LOJ:paragraph number=1 -->",
        "<!-- loj: paragraph number=1 -->",
        "<!-- loj:paragraph number=1 number=1 -->",
        "<!-- loj:paragraph number=01 -->",
        "<!-- loj:paragraph number=1 unknown=2 -->",
        "<!-- loj:paragraph\nnumber=1 -->",
        "<!-- loj:paragraph number=1 --> trailing",
    ] {
        assert!(
            rule(
                &run(
                    valid("本文").replacen("<!-- loj:paragraph number=1 -->", marker, 1),
                    Level::L1
                ),
                "DIR-01"
            ),
            "{marker}"
        );
    }
}
#[test]
fn resource_limit_is_incomplete_not_passed() {
    let path = fixture();
    write_manifest(
        &path,
        &manifest(json!([{"path":"main.md","parent":"main"}])),
    );
    let f = fs::File::create(path.join("main.md")).unwrap();
    f.set_len(MAX_FILE_BYTES + 1).unwrap();
    let r = check(&path, Level::L0);
    assert_eq!(r["status"], "Incomplete");
    assert_eq!(r["exit_code"], 3);
}

#[test]
fn invalid_documents_stop_at_total_input_limit() {
    let path = fixture();
    let count = MAX_TOTAL_BYTES / MAX_FILE_BYTES as usize;
    let mut documents = Vec::new();
    for i in 0..count {
        let name = format!("{i}.md");
        // NUL-filled, individually within the file limit, cumulatively over budget.
        fs::File::create(path.join(&name))
            .unwrap()
            .set_len(MAX_FILE_BYTES)
            .unwrap();
        documents.push(json!({"path":name,"parent":"main"}));
    }
    // Accessing this would produce Error: loading must stop at budget exhaustion.
    documents.push(json!({"path":"must-not-read.md","parent":"main"}));
    write_manifest(&path, &manifest(json!(documents)));
    let r = check(&path, Level::L0);
    assert_eq!(r["status"], "Incomplete");
    assert_eq!(r["exit_code"], 3);
    assert_eq!(r["complete"], false);
    let sources = r["sources"].as_array().unwrap();
    let retained: usize = sources
        .iter()
        .map(|s| s["text"].as_str().unwrap().len())
        .sum();
    assert!(retained <= MAX_TOTAL_BYTES);
    assert_eq!(
        sources.len(),
        count,
        "manifest plus only the fitting invalid documents"
    );
}
#[cfg(unix)]
#[test]
fn links_and_physical_duplicates_are_rejected() {
    use std::os::unix::fs::symlink;
    let path = fixture();
    fs::write(path.join("main.md"), valid("本文")).unwrap();
    fs::hard_link(path.join("main.md"), path.join("hard.md")).unwrap();
    symlink(path.join("main.md"), path.join("sym.md")).unwrap();
    for name in ["hard.md", "sym.md"] {
        write_manifest(
            &path,
            &manifest(json!([{"path":"main.md","parent":"main"},{"path":name,"parent":"main"}])),
        );
        assert!(rule(&check(&path, Level::L0), "MAN-02"));
    }
}

#[cfg(windows)]
#[test]
fn windows_hardlinks_and_junctions_are_rejected() {
    // Use the OS temporary filesystem: a workspace can be on exFAT without hardlinks.
    let path = std::env::temp_dir().join(format!("loj-v1-links-{}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    let target = path.join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("main.md"), valid("本文")).unwrap();
    fs::hard_link(target.join("main.md"), path.join("hard.md")).unwrap();
    write_manifest(
        &path,
        &manifest(
            json!([{"path":"target/main.md","parent":"main"},{"path":"hard.md","parent":"main"}]),
        ),
    );
    assert!(rule(&check(&path, Level::L0), "MAN-02"));
    let junction = path.join("junction");
    let out = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&target)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    write_manifest(
        &path,
        &manifest(json!([{"path":"junction/main.md","parent":"main"}])),
    );
    assert!(rule(&check(&path, Level::L0), "MAN-02"));
}

/// The builder reads manifest fields with `expect`; the loader returns
/// before it for any manifest that its schema refuses.
#[test]
fn a_manifest_that_fails_its_schema_is_reported_and_never_built() {
    let law = r#""law": {"id": "20990101-act-1", "title": "架空試験法", "type": "act",
        "number": "架空法律第一号", "promulgatedAt": "2099-01-01", "language": "ja"}"#;
    let cases = [
        r#"{"schemaVersion": "loj/v1alpha1", "law": {"id": "20990101-act-1"},
            "documents": [{"path": "main.md", "parent": "main"}]}"#
            .to_owned(),
        r#"{"schemaVersion": "loj/v1alpha1", "law": {"title": "架空試験法"},
            "documents": [{"path": "main.md", "parent": "main"}]}"#
            .to_owned(),
        format!(r#"{{"schemaVersion": "loj/v1alpha1", {law}}}"#),
        format!(r#"{{"schemaVersion": "loj/v1alpha1", {law}, "documents": "main.md"}}"#),
        format!(
            r#"{{"schemaVersion": "loj/v1alpha1", {law}, "documents": [{{"parent": "main"}}]}}"#
        ),
        format!(
            r#"{{"schemaVersion": "loj/v1alpha1", {law}, "documents": [{{"path": "main.md"}}]}}"#
        ),
        format!(
            r#"{{"schemaVersion": "loj/v1alpha1", {law}, "documents": [{{"path": 1, "parent": 2}}]}}"#
        ),
        format!(
            r#"{{"schemaVersion": "loj/v1alpha1", {law},
            "documents": [{{"path": "main.md", "parent": "main"}}],
            "supplements": [{{"title": "附則"}}]}}"#
        ),
        format!(
            r#"{{"schemaVersion": "loj/v1alpha1", {law},
            "documents": [{{"path": "main.md", "parent": "main"}}],
            "appendices": [{{"key": "table-1"}}]}}"#
        ),
    ];
    for (i, manifest) in cases.iter().enumerate() {
        let dir =
            std::env::temp_dir().join(format!("loj-v1-schema-refused-{}-{i}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("law.yaml"), manifest).unwrap();
        std::fs::write(dir.join("main.md"), "# 第一条\n").unwrap();
        let report = check(&dir, Level::L1);
        std::fs::remove_dir_all(&dir).unwrap();
        let diagnostics = report["diagnostics"].as_array().unwrap();
        assert!(!diagnostics.is_empty(), "{manifest}");
        assert!(
            diagnostics.iter().all(|d| d["rule"] == "MAN-01"),
            "{manifest}: {report}"
        );
        assert!(report.get("ast").is_none_or(|a| a.is_null()), "{manifest}");
        assert_ne!(report["exit_code"], 0, "{manifest}");
    }
}
