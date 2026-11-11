//! Property tests of the entry points that read untrusted input.
//!
//! Two properties are checked on generated input: the entry point returns
//! (a panic fails the test), and every position it reports is a range of the
//! file it names, on character boundaries. The inputs are synthetic.
//!
//! The seed is fixed, so a run gives the same result every time and an
//! unrelated change does not fail by chance. To look at other inputs, run
//! with `PROPTEST_RNG_SEED=<number>` and, for more of them,
//! `PROPTEST_CASES=3000`. A failure prints the minimal input; add it as an
//! ordinary test with the fix, since nothing is written to a regression file.
// Test helpers outside `#[test]` functions may unwrap, and offsets read
// back from JSON are cast for slicing: a failure here fails the test.
#![allow(clippy::unwrap_used, clippy::cast_possible_truncation)]

use law_semantic::common::{strict_text, yaml, Check};
use proptest::prelude::*;
use proptest::test_runner::RngSeed;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// The seed of every property unless `PROPTEST_RNG_SEED` is set.
const SEED: u64 = 0x10a_2026;

/// `cases` runs from the fixed seed. The environment variables of proptest
/// take precedence, and no regression file is written.
fn config(cases: u32) -> ProptestConfig {
    // The default already holds what the environment asks for.
    let mut config = ProptestConfig::default();
    if std::env::var_os("PROPTEST_CASES").is_none() {
        config.cases = cases;
    }
    if std::env::var_os("PROPTEST_RNG_SEED").is_none() {
        config.rng_seed = RngSeed::Fixed(SEED);
    }
    config.failure_persistence = None;
    config
}

/// Lines that the builders give a meaning to, valid and nearly valid.
const LINES: &[&str] = &[
    "# 第一条",
    "# 第一条（目的）",
    "# 第二条の二",
    "# 第二条ノ二",
    "# 第１０条",
    "# 第一章　総則",
    "## 第一節 通則",
    "# 第一條",
    "# 附則",
    "# 附　則",
    "# 附 則",
    "# 別表一",
    "# 第一条 ",
    "#第一条",
    "# 第一条 *強調*",
    "<!-- loj:paragraph number=1 -->",
    "<!-- loj:paragraph number=2 numbering=implicit -->",
    "<!-- loj:end-paragraph -->",
    "<!-- loj:item number=1 -->",
    "<!-- loj:end-item -->",
    "<!-- loj:subitem number=1 level=1 -->",
    "<!-- loj:subitem number=1 level=2 -->",
    "<!-- loj:end-subitem -->",
    "<!-- loj:table number=1 -->",
    "<!-- loj:end-table -->",
    "<!-- loj:row key=k3v9 -->",
    "<!-- loj:end-row -->",
    "<!-- loj:cell -->",
    "<!-- loj:cell colspan=2 -->",
    "<!-- loj:end-cell -->",
    "<!-- loj:preamble -->",
    "<!-- loj:end-preamble -->",
    "<!-- loj:enact-statement -->",
    "<!-- loj:end-enact-statement -->",
    "<!-- loj:paragraph -->",
    "<!-- loj:paragraph number=01 -->",
    "<!-- loj:unknown -->",
    "<!-- 注記 -->",
    "１　この法律は、架空の雲を保管する。",
    "２　前項の雲は、\u{e0100}異体字と葛\u{fe00}を含む。",
    "一　号の本文",
    "イ　細分の本文",
    "削除",
    "　",
    "",
    "```",
    "~~~text",
    "<pre>",
    "</pre>",
    "<script>",
    "> 引用",
    "- 箇条",
    "    字下げ",
    "| 表 | 見出し |",
    "| --- | --- |",
    "[条](#第一条)",
    "[外](https://example.invalid/)",
    "[別](other.md#第一条)",
    "か\u{3099}〓\u{fffd}",
    "\u{feff}",
    "a\u{0}b",
];

fn line() -> impl Strategy<Value = String> {
    prop_oneof![
        8 => prop::sample::select(LINES).prop_map(str::to_owned),
        1 => ".{0,12}",
        1 => "[#<>!\\-\\[\\]()`~|第条章附則のノ一二三１２ 　:=a-z0-9]{0,24}",
    ]
}

/// A Markdown source: lines joined by LF, CRLF or, rarely, a bare CR, with or
/// without a final line ending.
fn markdown() -> impl Strategy<Value = String> {
    let ending = prop_oneof![6 => Just("\n"), 3 => Just("\r\n"), 1 => Just("\r")];
    (
        prop::collection::vec((line(), ending), 0..40),
        any::<bool>(),
    )
        .prop_map(|(lines, last)| {
            let mut s = String::new();
            let count = lines.len();
            for (i, (line, ending)) in lines.into_iter().enumerate() {
                s.push_str(&line);
                if i + 1 < count || last {
                    s.push_str(ending);
                }
            }
            s
        })
}

/// A directory of its own for one case, removed when the case ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "loj-properties-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
    fn write(&self, name: &str, bytes: &[u8]) {
        std::fs::write(self.0.join(name), bytes).unwrap();
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn manifest(version: &str) -> String {
    format!(
        r#"{{"schemaVersion": "{version}",
"law": {{"id": "20990101-act-1", "title": "架空試験法（実在しないサンプル）", "type": "act",
  "number": "架空法律第一号", "promulgatedAt": "2099-01-01", "language": "ja"}},
"documents": [
  {{"path": "main.md", "parent": "main"}},
  {{"path": "main2.md", "parent": "main"}},
  {{"path": "original.md", "parent": "supplement/original"}},
  {{"path": "table.md", "parent": "appendix/table-1"}},
  {{"path": "table2.md", "parent": "appendix/table-1"}}],
"supplements": [{{"key": "original", "title": "制定附則"}}],
"appendices": [{{"key": "table-1", "title": "別表一"}}]}}
"#
    )
}

const FILES: [&str; 5] = [
    "main.md",
    "main2.md",
    "original.md",
    "table.md",
    "table2.md",
];

/// The spans of one record of `PackageCheck::positions`. A record is either
/// the position of a node (`{nodeId, path, start, end}`: the record itself is
/// the span) or a run of text (`{nodeId, contentIndex, text, spans}`).
fn position_spans(record: &Value) -> Vec<&Value> {
    match record["spans"].as_array() {
        Some(spans) => spans.iter().collect(),
        None => vec![record],
    }
}

/// `start..end` is a range of `text` on character boundaries.
fn assert_span(text: &str, start: u64, end: u64, what: &str) {
    let (start, end) = (start as usize, end as usize);
    assert!(start <= end && end <= text.len(), "{what}: {start}..{end}");
    assert!(
        text.is_char_boundary(start) && text.is_char_boundary(end),
        "{what}: {start}..{end} splits a character"
    );
}

proptest! {
    // In memory: a few hundred cases take well under a second.
    #![proptest_config(config(256))]

    #[test]
    fn strict_text_keeps_the_bytes_or_refuses(bytes in prop::collection::vec(any::<u8>(), 0..200)) {
        if let Ok(text) = strict_text(bytes.clone()) {
            prop_assert_eq!(text.as_bytes(), &bytes[..]);
            let clean = !text.is_empty() && !text.starts_with('\u{feff}') && !text.contains('\0');
            prop_assert!(clean);
            let b = text.as_bytes();
            for (i, c) in b.iter().enumerate() {
                prop_assert!(*c != b'\r' || b.get(i + 1) == Some(&b'\n'));
            }
        }
    }

    #[test]
    fn strict_text_accepts_what_it_should(source in markdown()) {
        let expected = !source.is_empty()
            && !source.starts_with('\u{feff}')
            && !source.contains('\0')
            && !source.replace("\r\n", "").contains('\r');
        prop_assert_eq!(strict_text(source.clone().into_bytes()).is_ok(), expected);
    }

    #[test]
    fn yaml_returns_for_any_text(source in prop_oneof![
        ".{0,80}",
        "[a-c:\\- \\n\\[\\]{},'\"#&*!|>%@`~?<]{0,80}",
    ]) {
        if let Ok(value) = yaml::parse(&source) {
            // Whatever is accepted is JSON data: it prints and reads back.
            let again: Value = serde_json::from_str(&value.to_string()).unwrap();
            prop_assert_eq!(again, value);
        }
    }

    #[test]
    fn dates_read_back_as_written(source in prop_oneof![
        "[0-9]{4}-[0-9]{2}-[0-9]{2}",
        "[0-9０-９+\\- ]{10}",
        ".{0,12}",
    ]) {
        if let Some(date) = law_semantic::common::time::Date::parse(&source) {
            prop_assert_eq!(date.to_iso(), source);
            prop_assert_eq!(date.add_days(1).add_days(-1), date);
        }
    }

    #[test]
    fn references_and_numbers_return_for_any_text(source in prop_oneof![
        ".{0,60}",
        "loj:[a-z0-9/@#:.\\-]{0,60}",
        "[一二三十百千万億兆〇零0-9０-９のノ]{0,24}",
    ]) {
        let _ = law_semantic::fixref::parse(&source);
        if let Ok(parts) = law_semantic::normalize_number(&source) {
            prop_assert!(!parts.is_empty() && parts.iter().all(|p| *p >= 1));
        }
    }

    #[test]
    fn legacy_mapping_reports_positions_inside_the_source(source in markdown()) {
        let profile = law_semantic::law_of_japan_profile();
        // `Err` is for an unusable profile only, and this is the embedded one:
        // whatever is wrong with the Markdown is a diagnostic inside the AST.
        let mut ast = law_semantic::map_markdown(&source, "t.md", "t", &profile).unwrap();
        fn spans(v: &Value, source: &str) {
            match v {
                Value::Object(m) => {
                    if let (Some(start), Some(end)) = (m.get("start_byte"), m.get("end_byte")) {
                        assert_span(source, start.as_u64().unwrap(), end.as_u64().unwrap(), "legacy");
                    }
                    m.values().for_each(|x| spans(x, source));
                }
                Value::Array(a) => a.iter().for_each(|x| spans(x, source)),
                _ => {}
            }
        }
        spans(&ast, &source);
        // Validation reports what is wrong with the AST as diagnostics.
        prop_assert!(law_semantic::validate_document(&ast).is_ok());
        // These two refuse an AST that does not satisfy the schema (the mapper
        // builds one from an empty source, for example) with `Err`; they must
        // not panic, and they must agree on which ASTs they accept.
        let conforming = law_semantic::conforms(&ast).unwrap();
        prop_assert_eq!(law_semantic::reference_coverage(&ast).is_ok(), conforming);
        prop_assert_eq!(law_semantic::resolve_references(&mut ast).is_ok(), conforming);
    }
}

proptest! {
    // Each case writes a package to disk: fewer, to keep the suite at seconds.
    #![proptest_config(config(96))]

    #[test]
    fn v1alpha2_reports_positions_inside_the_files(
        files in prop::collection::vec(markdown(), 5),
    ) {
        let dir = Scratch::new();
        let manifest = manifest("loj/v1alpha2");
        dir.write("law.yaml", manifest.as_bytes());
        for (name, text) in FILES.iter().zip(&files) {
            dir.write(name, text.as_bytes());
        }
        let checked = law_semantic::v1alpha2::check(&dir.0, &[Check::L0, Check::L1]);
        let text = |path: &str| match FILES.iter().position(|f| *f == path) {
            Some(i) => Some(files[i].as_str()),
            None => (path == "law.yaml").then_some(manifest.as_str()),
        };
        for d in &checked.report.diagnostics {
            let Some(location) = &d.location else { continue };
            let path = location["path"].as_str().unwrap();
            if let (Some(source), Some(start), Some(end)) =
                (text(path), location["start"].as_u64(), location["end"].as_u64())
            {
                assert_span(source, start, end, &format!("{} in {path}", d.rule));
            }
        }
        for p in &checked.positions {
            for span in position_spans(p) {
                let path = span["path"].as_str().unwrap();
                let source = text(path);
                prop_assert!(source.is_some(), "a position names an unknown file: {}", path);
                assert_span(
                    source.unwrap(),
                    span["start"].as_u64().unwrap(),
                    span["end"].as_u64().unwrap(),
                    "position",
                );
            }
        }
        // AST-02: an AST is returned only when L1 passed.
        prop_assert_eq!(
            checked.ast.is_some(),
            checked.report.result(Check::L1) == Some(law_semantic::common::ResultValue::Pass)
        );
    }

    #[test]
    fn v1alpha1_reports_positions_inside_the_files(
        files in prop::collection::vec(markdown(), 5),
    ) {
        let dir = Scratch::new();
        let manifest = manifest("loj/v1alpha1");
        dir.write("law.yaml", manifest.as_bytes());
        for (name, text) in FILES.iter().zip(&files) {
            dir.write(name, text.as_bytes());
        }
        let report = law_semantic::v1alpha1::check(&dir.0, law_semantic::v1alpha1::Level::L1);
        let text = |path: &str| match FILES.iter().position(|f| *f == path) {
            Some(i) => Some(files[i].as_str()),
            None => (path == "law.yaml").then_some(manifest.as_str()),
        };
        for d in report["diagnostics"].as_array().into_iter().flatten() {
            let path = d["file"].as_str().unwrap();
            if let (Some(source), Some(start), Some(end)) =
                (text(path), d["span"]["start"].as_u64(), d["span"]["end"].as_u64())
            {
                assert_span(source, start, end, &format!("{} in {path}", d["rule"]));
            }
        }
        prop_assert!(report["status"].is_string());
    }

    #[test]
    fn a_manifest_of_any_bytes_is_refused_not_trusted(
        bytes in prop_oneof![
            prop::collection::vec(any::<u8>(), 0..120),
            "[a-z:\\- \\n\\[\\]{},'\"0-9./]{0,120}".prop_map(String::into_bytes),
        ],
    ) {
        let dir = Scratch::new();
        dir.write("law.yaml", &bytes);
        let checked = law_semantic::v1alpha2::check(&dir.0, &[Check::L0, Check::L1]);
        // Nothing passes: a manifest needs `schemaVersion`, `law` with six
        // fields and `documents`, in over 150 bytes, and at most 120 are generated.
        prop_assert!(checked.ast.is_none());
        prop_assert_ne!(checked.report.exit_code(), 0);
        let report = law_semantic::v1alpha1::check(&dir.0, law_semantic::v1alpha1::Level::L1);
        prop_assert_ne!(report["exit_code"].as_i64(), Some(0));
    }
}
