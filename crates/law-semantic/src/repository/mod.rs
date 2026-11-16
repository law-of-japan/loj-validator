//! One law repository: L0/L1 of every version package, L3 historyStructure
//! and evidence (VAL-05). Each result is judged independently (VAL-02).
pub(crate) mod glyphs;
pub(crate) mod history;

use crate::common::findings::Findings;
use crate::common::{load_yaml, Check, Class, Loaded, Report, ResultValue};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub const HISTORY: &str = "loj-history/v1alpha1";
pub const GLYPHS: &str = "loj-glyphs/v1alpha1";
pub(crate) const HS: Check = Check::HISTORY_STRUCTURE;
pub(crate) const EVIDENCE: Check = Check::EVIDENCE;

/// Read `history.yaml`. `None` with an incomplete/fail finding on error.
pub(crate) fn load_history(repo: &Path, f: &mut Findings) -> Option<Value> {
    match load_yaml(&repo.join("history.yaml")) {
        Loaded::Unreadable(e) => {
            f.unprocessable(
                HS,
                crate::common::Diagnostic::error(
                    "HIST-01",
                    HISTORY,
                    format!("history.yaml unreadable: {e}"),
                )
                .at_pointer("history.yaml", ""),
            );
            f.incomplete.insert(HS);
            f.incomplete.insert(EVIDENCE);
            None
        }
        Loaded::Invalid(e) => {
            f.error(HS, "HIST-01", HISTORY, e, "history.yaml", "");
            f.incomplete.insert(EVIDENCE);
            None
        }
        Loaded::Ok(v, _) if v["schemaVersion"] != HISTORY => {
            // VAL-04: unknown versions are unprocessable (incomplete), not read as this one.
            f.push(
                HS,
                crate::common::Diagnostic::error("HIST-01", HISTORY, "unsupported schemaVersion")
                    .with_code("loj-history/v1alpha1/unsupported-version")
                    .at_pointer("history.yaml", "/schemaVersion"),
            );
            f.incomplete.insert(HS);
            f.incomplete.insert(EVIDENCE);
            None
        }
        Loaded::Ok(v, _) => Some(v),
    }
}

/// Run the selected checks over a repository directory.
pub fn check(repo: &Path, checks: &[Check]) -> Report {
    let mut f = Findings::default();
    let mut report = Report::default();
    report
        .target
        .insert("repository".into(), json!(repo.to_string_lossy()));
    let wants = |c: Check| checks.contains(&c);
    let l3 = wants(HS) || wants(EVIDENCE);
    let h = load_history(repo, &mut f);
    // L0/L1 of each version package with text, and the ASTs for evidence.
    let mut asts: HashMap<String, Option<Value>> = HashMap::new();
    let mut package_results: Vec<(Check, ResultValue)> = Vec::new();
    let empty = Vec::new();
    let revisions = h
        .as_ref()
        .and_then(|h| h["revisions"].as_array())
        .unwrap_or(&empty)
        .clone();
    let mut paths = crate::common::documents::DocumentPaths::new(&repo.join("history.yaml")).ok();
    let record = h.as_ref().map(|h| h["record"]["id"].clone());
    for (i, r) in revisions.iter().enumerate() {
        let Some(id) = r["id"].as_str() else { continue };
        if !matches!(r["text"]["status"].as_str(), Some("complete" | "partial")) {
            continue;
        }
        if !history::valid_revision_id(id) {
            continue;
        }
        let path = history::package_path(r);
        let full = match paths
            .as_mut()
            .and_then(|paths| paths.resolve_yaml(&path).ok())
        {
            Some(full) => full,
            None => {
                f.incomplete.insert(HS);
                f.incomplete.insert(EVIDENCE);
                continue;
            }
        };
        if !full.is_file() {
            continue; // HIST-08
        }
        // HIST-02: law.id is the Record ID (read without requiring L1).
        if let (Loaded::Ok(m, _), Some(rid)) = (load_yaml(&full), &record) {
            if m["law"]["id"].is_string() && m["law"]["id"] != *rid {
                f.error(
                    HS,
                    "HIST-02",
                    HISTORY,
                    format!(
                        "{path}: law.id {} is not the Record ID {rid}",
                        m["law"]["id"]
                    ),
                    "history.yaml",
                    &format!("/revisions/{i}"),
                );
            }
        }
        let levels = [Check::L0, Check::L1];
        let pkg = crate::v1alpha2::check(&full, &levels);
        for c in levels {
            if let Some(v) = pkg.report.result(c) {
                package_results.push((c, v));
            }
        }
        let prefix = Path::new(&path)
            .parent()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        for mut d in pkg.report.diagnostics {
            if let Some(l) = d.location.as_mut() {
                for key in ["path", "file"] {
                    if let Some(p) = l[key].as_str() {
                        l[key] = json!(format!("{prefix}/{p}"));
                    }
                }
            }
            f.push(Check::L1, d);
        }
        asts.insert(id.to_owned(), pkg.ast);
    }
    for c in [Check::L0, Check::L1] {
        let r = package_results
            .iter()
            .filter(|(k, _)| *k == c)
            .fold(ResultValue::NotApplicable, |acc, (_, v)| acc.worst(*v));
        report.set(c, r);
    }
    if let Some(h) = &h {
        if l3 {
            history::check(h, repo, &mut f);
            let sources: HashSet<String> = h["sources"]
                .as_array()
                .unwrap_or(&empty)
                .iter()
                .filter_map(|s| s["id"].as_str().map(str::to_owned))
                .collect();
            for r in &revisions {
                let Some(id) = r["id"].as_str() else { continue };
                if !matches!(r["text"]["status"].as_str(), Some("complete" | "partial")) {
                    continue;
                }
                let Some(ast) = asts.get(id) else { continue };
                let notes = format!("versions/{id}/glyphs.yaml");
                glyphs::check(
                    repo,
                    id,
                    repo.join(&notes).is_file().then_some(notes.as_str()),
                    ast.as_ref(),
                    &sources,
                    &mut f,
                );
            }
            scope_nodes(h, &asts, &mut f);
            row_key_reuse(h, &asts, &mut f);
        }
    }
    if wants(Check::REBUILD) {
        crate::rebuild::check(repo, h.as_ref(), &asts, &mut f);
    }
    let mut out = Report::default();
    for c in [HS, EVIDENCE, Check::REBUILD] {
        out.set(c, f.result(c));
    }
    for (c, r) in out.results {
        report.set(c, r);
    }
    for c in checks {
        if report.result(*c).is_none() {
            report.set(*c, ResultValue::NotApplicable);
        }
    }
    report.select(checks);
    report.diagnostics = f.items.into_iter().map(|(_, d)| d).collect();
    if let Some(id) = record.and_then(|id| id.as_str().map(str::to_owned)) {
        report.records.push((id, report.clone()));
    }
    report
}

/// HIST-13: branch-local row lifetimes; skipped revisions do not delete rows.
fn row_key_reuse(h: &Value, asts: &HashMap<String, Option<Value>>, f: &mut Findings) {
    fn rows(ast: &Value) -> HashSet<String> {
        let mut found = HashSet::new();
        let mut pending = vec![&ast["root"]];
        while let Some(node) = pending.pop() {
            if node["kind"] == "Row" {
                if let Some(id) = node["id"].as_str() {
                    found.insert(id.to_owned());
                }
            }
            pending.extend(history::list(&node["content"]));
        }
        found
    }
    enum Visit {
        Enter(usize),
        Exit(Vec<(String, Option<bool>)>),
    }
    let revisions = history::list(&h["revisions"]);
    let indices: HashMap<&str, usize> = revisions
        .iter()
        .enumerate()
        .filter_map(|(i, r)| r["id"].as_str().map(|id| (id, i)))
        .collect();
    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut stack = Vec::new();
    for (i, revision) in revisions.iter().enumerate() {
        if let Some(parent) = revision["basedOn"].as_str().and_then(|id| indices.get(id)) {
            children.entry(*parent).or_default().push(i);
        } else {
            stack.push(Visit::Enter(i));
        }
    }
    let mut seen = HashSet::new();
    // true = present at the previous comparable revision; false = deleted.
    let mut state: HashMap<String, bool> = HashMap::new();
    while let Some(visit) = stack.pop() {
        let i = match visit {
            Visit::Exit(changes) => {
                for (key, old) in changes.into_iter().rev() {
                    if let Some(old) = old {
                        state.insert(key, old);
                    } else {
                        state.remove(&key);
                    }
                }
                continue;
            }
            Visit::Enter(i) => i,
        };
        if !seen.insert(i) {
            continue;
        }
        let revision = &revisions[i];
        let mut changes = Vec::new();
        if revision["status"] == "active" && revision["text"]["status"] == "complete" {
            if let Some(Some(ast)) = revision["id"].as_str().and_then(|id| asts.get(id)) {
                let keys = rows(ast);
                for (key, present) in &mut state {
                    if *present && !keys.contains(key) {
                        changes.push((key.clone(), Some(true)));
                        *present = false;
                    }
                }
                for key in keys {
                    if state.get(&key) == Some(&false) {
                        f.error(
                            HS,
                            "HIST-13",
                            HISTORY,
                            format!("deleted table row key reused: {key}"),
                            "history.yaml",
                            &format!("/revisions/{i}"),
                        );
                    }
                    if state.get(&key) != Some(&true) {
                        changes.push((key.clone(), state.insert(key, true)));
                    }
                }
            }
        }
        stack.push(Visit::Exit(changes));
        if let Some(children) = children.get(&i) {
            stack.extend(children.iter().rev().map(|i| Visit::Enter(*i)));
        }
    }
}

/// TYPE-07/08: Scope.nodes of a basis item exist in the package of the revision.
fn scope_nodes(h: &Value, asts: &HashMap<String, Option<Value>>, f: &mut Findings) {
    let empty = Vec::new();
    let relations = h["relations"].as_array().unwrap_or(&empty);
    for (i, r) in h["revisions"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .enumerate()
    {
        let id = r["id"].as_str().unwrap_or("");
        for b in r["basis"].as_array().unwrap_or(&empty) {
            let Some(rel) = relations.iter().find(|x| x["id"] == b["relation"]) else {
                continue;
            };
            for item in b["items"].as_array().unwrap_or(&empty) {
                let Some(it) = rel["items"]
                    .as_array()
                    .and_then(|a| a.iter().find(|x| x["id"] == *item))
                else {
                    continue;
                };
                let Some(nodes) = it["scope"]["nodes"].as_array() else {
                    continue;
                };
                let ids = match asts.get(id) {
                    Some(Some(ast)) => glyphs::Texts::from_ast(ast),
                    _ => {
                        f.note(
                            EVIDENCE,
                            Class::Undetermined,
                            "TYPE-07",
                            crate::common::COMMON,
                            format!("scope nodes of revision {id} not checked: no valid AST"),
                            "history.yaml",
                            &format!("/revisions/{i}"),
                        );
                        continue;
                    }
                };
                for n in nodes {
                    if let Some(n) = n.as_str() {
                        if !ids.contains(n) {
                            f.error(
                                EVIDENCE,
                                "TYPE-07",
                                crate::common::COMMON,
                                format!("scope node {n} is not in revision {id}"),
                                "history.yaml",
                                &format!("/revisions/{i}/basis"),
                            );
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn row_lifetimes_follow_branches_and_skip_partial_snapshots() {
        let revision = |id: &str, parent: Option<&str>, status: &str| json!({"id":id,"status":"active","basedOn":parent,"text":{"status":status}});
        let history = json!({"revisions":[revision("a",None,"complete"),revision("b",Some("a"),"complete"),revision("c",Some("b"),"partial"),revision("d",Some("c"),"complete"),revision("e",Some("a"),"complete")]});
        let rows = |present| {
            Some(
                json!({"root":{"kind":"Law","content":if present {vec![json!({"kind":"Row","id":"law/table/1/row/k3v9"})]} else {vec![]}}}),
            )
        };
        let asts = HashMap::from([
            ("a".into(), rows(true)),
            ("b".into(), rows(false)),
            ("c".into(), rows(true)),
            ("d".into(), rows(true)),
            ("e".into(), rows(true)),
        ]);
        let mut f = Findings::default();
        row_key_reuse(&history, &asts, &mut f);
        let findings: Vec<_> = f
            .items
            .iter()
            .filter(|(_, d)| d.rule == "HIST-13")
            .collect();
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].1.location.as_ref().unwrap()["pointer"],
            "/revisions/3"
        );
    }
}
