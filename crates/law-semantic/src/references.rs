// Legacy pipeline: its unwraps read an AST that the entry points have
// checked against the schema. law-of-japan/loj-validator#7 tracks them.
#![allow(clippy::unwrap_used)]
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// One character that a heading slug keeps.
static SLUG_CHAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\p{L}\p{M}\p{N}_-]$").expect("constant regex"));

const NUMERAL: &str = "[一二三四五六七八九十百千万億兆〇零0-9０-９]+";
/// An article label such as `第十二条の二`: number `n`, branch numbers `b`.
static ARTICLE_LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        "^第(?P<n>{NUMERAL})条(?P<b>(?:[のノ]{NUMERAL})*)$"
    ))
    .expect("constant regex")
});

/// GitHub's documented basic heading rule; corpus rendering is tested separately.
pub fn slug(text: &str, used: &mut HashSet<String>) -> String {
    let base: String = text
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| {
            if c == ' ' {
                Some('-')
            } else if SLUG_CHAR.is_match(c.encode_utf8(&mut [0; 4])) {
                Some(c)
            } else {
                None
            }
        })
        .collect();
    let mut candidate = base.clone();
    let mut n = 0;
    while used.contains(&candidate) {
        n += 1;
        candidate = format!("{base}-{n}");
    }
    used.insert(candidate.clone());
    candidate
}

fn decode(fragment: &str) -> Result<String, String> {
    let mut out = Vec::new();
    let bytes = fragment.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err("incomplete percent escape".into());
            }
            let h = (bytes[i + 1] as char)
                .to_digit(16)
                .ok_or("invalid percent escape")?;
            let l = (bytes[i + 2] as char)
                .to_digit(16)
                .ok_or("invalid percent escape")?;
            out.push(u8::try_from(h * 16 + l).map_err(|_| "invalid percent escape")?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "fragment is not valid UTF-8".into())
}

fn nodes(n: &Value, index: &mut HashMap<String, Vec<Value>>) {
    index
        .entry(n["id"].as_str().unwrap().into())
        .or_default()
        .push(json!({
        "id":n["id"],"kind":n["kind"],"number":n["number"],"origins":n["origins"]}));
    for c in n["children"].as_array().unwrap() {
        nodes(c, index);
    }
}

fn diagnostic(
    code: &str,
    message: String,
    node_id: &str,
    r: &Value,
    related: Value,
    severity: &str,
) -> Value {
    json!({"code":code,"severity":severity,"message":message,"rule_id":"semantic/local-reference",
        "node_id":node_id,"primary_span":r["origins"][0],"related_spans":related})
}

fn visit(
    n: &mut Value,
    index: &HashMap<String, Vec<Value>>,
    anchors: &HashMap<(String, String), Vec<Value>>,
    issues: &mut Vec<Value>,
) {
    let node_id = n["id"].as_str().unwrap().to_owned();
    for r in n["references"].as_array_mut().unwrap() {
        let uri = r["target_uri"].as_str().unwrap().to_owned();
        r["target_id"] = Value::Null;
        r["resolution"] = json!("unresolved");
        if !uri.starts_with('#') {
            r["resolution"] = json!(
                if uri.starts_with("https://") || uri.starts_with("http://") {
                    "external"
                } else {
                    "unsupported"
                }
            );
            issues.push(diagnostic(
                "REF_OUT_OF_SCOPE",
                format!("not resolved by same-file policy: {uri}"),
                &node_id,
                r,
                json!([]),
                "warning",
            ));
            continue;
        }
        let fragment = match decode(&uri[1..]) {
            Ok(s) => s,
            Err(msg) => {
                issues.push(diagnostic(
                    "REF_FRAGMENT_INVALID",
                    msg,
                    &node_id,
                    r,
                    json!([]),
                    "error",
                ));
                continue;
            }
        };
        let source = r["origins"][0]["source"].as_str().unwrap().to_owned();
        let Some(matches) = anchors.get(&(source, fragment.clone())) else {
            issues.push(diagnostic(
                "REF_TARGET_MISSING",
                format!("missing anchor: {fragment}"),
                &node_id,
                r,
                json!([]),
                "error",
            ));
            continue;
        };
        let related: Vec<Value> = matches
            .iter()
            .flat_map(|a| a["origins"].as_array().unwrap().clone())
            .collect();
        if matches.len() != 1 {
            issues.push(diagnostic(
                "REF_AMBIGUOUS",
                format!("multiple anchors: {fragment}"),
                &node_id,
                r,
                json!(related),
                "error",
            ));
            continue;
        }
        let a = &matches[0];
        let Some(target_id) = a["node_id"].as_str() else {
            issues.push(diagnostic(
                "REF_TARGET_UNTYPED",
                format!("anchor has no semantic binding: {fragment}"),
                &node_id,
                r,
                json!(related),
                "error",
            ));
            continue;
        };
        let Some(targets) = index.get(target_id) else {
            issues.push(diagnostic(
                "REF_TARGET_MISSING",
                format!("anchor refers to unknown node: {target_id}"),
                &node_id,
                r,
                json!(related),
                "error",
            ));
            continue;
        };
        if targets.len() != 1 {
            let spans: Vec<Value> = targets
                .iter()
                .flat_map(|t| t["origins"].as_array().unwrap().clone())
                .collect();
            issues.push(diagnostic(
                "REF_AMBIGUOUS",
                format!("semantic id is not unique: {target_id}"),
                &node_id,
                r,
                json!(spans),
                "error",
            ));
            continue;
        }
        let target = &targets[0];
        if !r["expected_kinds"]
            .as_array()
            .unwrap()
            .contains(&target["kind"])
        {
            issues.push(diagnostic(
                "REF_TYPE_MISMATCH",
                format!("expected {}, got {}", r["expected_kinds"], target["kind"]),
                &node_id,
                r,
                json!(related),
                "error",
            ));
            continue;
        }
        let label = r["label"].as_str().unwrap();
        if let Some(c) = ARTICLE_LABEL.captures(label) {
            let display = format!("{}{}", &c["n"], &c["b"]);
            match crate::number::normalize(&display) {
                Ok(parts) if json!(parts) == target["number"]["parts"] => {}
                Ok(_) => {
                    issues.push(diagnostic(
                        "REF_LABEL_MISMATCH",
                        format!("label number differs from target: {label}"),
                        &node_id,
                        r,
                        json!(related),
                        "error",
                    ));
                    continue;
                }
                Err(msg) => {
                    issues.push(diagnostic(
                        "REF_LABEL_INVALID",
                        msg,
                        &node_id,
                        r,
                        json!(related),
                        "error",
                    ));
                    continue;
                }
            }
        }
        r["target_id"] = json!(target_id);
        r["resolution"] = json!("resolved");
    }
    for child in n["children"].as_array_mut().unwrap() {
        visit(child, index, anchors, issues);
    }
}

/// Requires schema-valid input. Recompute, never trust supplied resolution flags.
/// Public callers use [`crate::resolve_references`], which checks the input.
pub(crate) fn resolve(data: &mut Value) -> Vec<Value> {
    let mut index = HashMap::new();
    for n in data["laws"].as_array().unwrap() {
        nodes(n, &mut index);
    }
    let mut anchors: HashMap<(String, String), Vec<Value>> = HashMap::new();
    for a in data["anchors"].as_array().unwrap() {
        let source = a["origins"][0]["source"].as_str().unwrap().to_owned();
        anchors
            .entry((source, a["name"].as_str().unwrap().into()))
            .or_default()
            .push(a.clone());
    }
    let mut issues = vec![];
    for n in data["laws"].as_array_mut().unwrap() {
        visit(n, &index, &anchors, &mut issues);
    }
    issues
}

/// Requires schema-valid input; public callers use [`crate::reference_coverage`].
pub(crate) fn coverage(data: &Value) -> Value {
    let mut copy = data.clone();
    let diagnostics = resolve(&mut copy);
    fn count(n: &Value, total: &mut usize, resolved: &mut usize, excluded: &mut usize) {
        for r in n["references"].as_array().unwrap() {
            *total += 1;
            if r["resolution"] == "resolved" {
                *resolved += 1;
            }
            if r["resolution"] == "external" || r["resolution"] == "unsupported" {
                *excluded += 1;
            }
        }
        for c in n["children"].as_array().unwrap() {
            count(c, total, resolved, excluded);
        }
    }
    let (mut total, mut resolved, mut excluded) = (0, 0, 0);
    for n in copy["laws"].as_array().unwrap() {
        count(n, &mut total, &mut resolved, &mut excluded);
    }
    json!({"policy":"same-file-fragment","total":total,"resolved":resolved,"out_of_scope":excluded,
        "failed":total-resolved-excluded,"complete":total==resolved,
        "warnings":diagnostics.into_iter().filter(|d|d["severity"]=="warning").collect::<Vec<_>>()})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basic_slugs_and_collisions() {
        let mut used = HashSet::new();
        for (heading, expected) in [
            ("第一条（目的）", "第一条目的"),
            ("第一章　総則", "第一章総則"),
            ("A  B!", "a--b"),
            ("A  B!", "a--b-1"),
            ("a--b-1", "a--b-1-1"),
        ] {
            assert_eq!(slug(heading, &mut used), expected);
        }
    }
    #[test]
    fn slugs_keep_marks_and_drop_other_symbols() {
        // U+3099 is a combining mark (Mn): kept. U+FE00 is a variation
        // selector (also Mn): kept. Punctuation, symbols and U+3000 are dropped.
        for (heading, expected) in [
            ("か\u{3099}き", "か\u{3099}き"),
            ("葛\u{fe00}飾", "葛\u{fe00}飾"),
            ("第１条_a-b", "第１条_a-b"),
            ("a\u{3000}b・c〔d〕", "abcd"),
            ("😀x", "x"),
        ] {
            assert_eq!(slug(heading, &mut HashSet::new()), expected, "{heading}");
        }
    }
    #[test]
    fn article_labels() {
        let c = ARTICLE_LABEL.captures("第十二条の二ノ３").unwrap();
        assert_eq!((&c["n"], &c["b"]), ("十二", "の二ノ３"));
        let c = ARTICLE_LABEL.captures("第１０条").unwrap();
        assert_eq!((&c["n"], &c["b"]), ("１０", ""));
        for bad in [
            "第条",
            "第一条の",
            "第一条 ",
            "第一項",
            "一条",
            "第一条の二\n",
        ] {
            assert!(!ARTICLE_LABEL.is_match(bad), "{bad:?}");
        }
    }
    #[test]
    fn fragments() {
        assert_eq!(decode("%E7%AC%AC%E4%BA%8C%E6%9D%A1").unwrap(), "第二条");
        assert_eq!(decode("a+b").unwrap(), "a+b");
        for s in ["%", "%G1", "%1", "%FF"] {
            assert!(decode(s).is_err());
        }
    }
}
