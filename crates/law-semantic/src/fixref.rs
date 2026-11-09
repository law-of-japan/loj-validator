//! Resolution of fixed and Revision references (common/v1alpha1 FIXREF-02..07),
//! reported apart from the conformance levels.
use crate::common::findings::Findings;
use crate::common::{Check, Report, COMMON};
use crate::git::Bundle;
use crate::registry::{self, Resolution};
use serde_json::{json, Map, Value};
use std::path::Path;

const RF: Check = Check::REFERENCE;

/// FIXREF-02 string form.
#[derive(Debug, PartialEq, Eq)]
pub struct Parsed {
    pub space: String,
    pub record: String,
    pub revision: String,
    pub commit: Option<String>,
    pub node: Option<String>,
}

pub fn parse(s: &str) -> Result<Parsed, String> {
    let rest = s
        .strip_prefix("loj:")
        .ok_or("a reference starts with loj:")?;
    let (rest, node) = match rest.split_once('#') {
        Some((a, n)) => (a, Some(n.to_owned())),
        None => (rest, None),
    };
    let (path, commit) = match rest.split_once('@') {
        Some((a, c)) => (a, Some(c.to_owned())),
        None => (rest, None),
    };
    let parts: Vec<&str> = path.split('/').collect();
    let [space, record, revision] = parts[..] else {
        return Err("expected loj:<space>/<record>/<revision>".into());
    };
    if [space, record, revision].iter().any(|p| p.is_empty())
        || node
            .as_deref()
            .is_some_and(|n| n.is_empty() || n.contains(['@', '#']))
        || commit.as_deref().is_some_and(|c| c.contains(['@', '#']))
    {
        return Err("empty or invalid component".into());
    }
    Ok(Parsed {
        space: space.into(),
        record: record.into(),
        revision: revision.into(),
        commit,
        node,
    })
}

fn full_hash(c: &str) -> bool {
    (c.len() == 40 || c.len() == 64)
        && c.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn check(bundle: &Path, registry_dir: &Path, reference: &str) -> Report {
    let mut f = Findings::default();
    f.applicable.insert(RF);
    let mut resolution = json!({"result": "error"});
    resolve(bundle, registry_dir, reference, &mut f, &mut resolution);
    match resolution["result"].as_str() {
        Some("notFetched") => {
            f.incomplete.insert(RF);
        }
        Some("error") if !f.failed(RF) => {
            f.error(
                RF,
                "FIXREF-06",
                COMMON,
                "reference not resolved",
                reference,
                "",
            );
        }
        _ => {}
    }
    let mut r = f.into_report(&[RF]);
    r.resolution = Some(resolution);
    r
}

fn resolve(bundle: &Path, registry_dir: &Path, reference: &str, f: &mut Findings, out: &mut Value) {
    let err = |f: &mut Findings, rule: &str, msg: String| {
        f.error(RF, rule, COMMON, msg, reference, "");
    };
    let p = match parse(reference) {
        Ok(p) => p,
        Err(e) => return err(f, "FIXREF-02", e),
    };
    // FIXREF-07: a node needs a commit.
    if p.node.is_some() && p.commit.is_none() {
        err(f, "FIXREF-07", "a node is given without @<commit>".into());
        return err(
            f,
            "FIXREF-06",
            "a reference without commit cannot address a node".into(),
        );
    }
    // FIXREF-04: full hashes only.
    if let Some(c) = &p.commit {
        if !full_hash(c) {
            err(f, "FIXREF-04", format!("{c} is not a full commit hash"));
            return err(f, "FIXREF-06", "commit cannot be identified".into());
        }
    }
    let mut scratch = Findings::default();
    let Some(reg) = registry::load(registry_dir, &mut scratch) else {
        *out = json!({"result": "notFetched"});
        return;
    };
    if p.space != reg.space {
        *out = json!({"result": "notFetched"});
        return;
    }
    let record = match reg.resolve(&json!({"id": p.record})) {
        Resolution::Resolved(id) => id,
        _ => {
            *out = json!({"result": "notFetched"});
            return;
        }
    };
    let Some(rec) = reg.records.get(&record) else {
        *out = json!({"result": "notFetched"});
        return;
    };
    let branch = rec["repository"]["canonicalBranch"]
        .as_str()
        .unwrap_or("main")
        .to_owned();
    match Bundle::open(bundle) {
        Ok(b) => resolve_in(&b, &p, &record, &branch, reference, f, out),
        Err(e) => not_fetched(f, out, reference, e),
    }
}

/// The repository could not be read (git missing or failing, an unusable tree):
/// the reference is not fetched, with the reason, and never reported as
/// unresolvable (VAL-04). `check` marks the check incomplete for every notFetched
/// result; the note only adds the reason. The note carries FIXREF-06, the rule
/// that defines the result values, notFetched among them.
fn not_fetched(f: &mut Findings, out: &mut Value, reference: &str, why: String) {
    f.note(RF, "undetermined", "FIXREF-06", COMMON, why, reference, "");
    *out = json!({"result": "notFetched"});
}

/// Resolve `p` in an opened bundle holding `record`, whose canonical branch is
/// `branch`.
fn resolve_in(
    b: &Bundle,
    p: &Parsed,
    record: &str,
    branch: &str,
    reference: &str,
    f: &mut Findings,
    out: &mut Value,
) {
    let err = |f: &mut Findings, rule: &str, msg: String| {
        f.error(RF, rule, COMMON, msg, reference, "");
    };
    let not_fetched =
        |f: &mut Findings, out: &mut Value, why: String| not_fetched(f, out, reference, why);
    let tip = match b.branch(branch) {
        Ok(Some(tip)) => tip,
        // The canonical branch is not in the bundle: the bundle does not hold the
        // record's history, so the reference is not fetched (no git failure to note).
        Ok(None) => {
            *out = json!({"result": "notFetched"});
            return;
        }
        Err(e) => return not_fetched(f, out, e),
    };
    let commit = match &p.commit {
        None => tip.clone(), // FIXREF-05: latest record version of the canonical branch
        Some(c) => match b.commit(c) {
            Ok(Some(full)) if full == *c => match b.is_ancestor(c, &tip) {
                Ok(true) => full,
                Ok(false) => {
                    return err(
                        f,
                        "FIXREF-04",
                        format!("{c} is not reachable from {branch}"),
                    );
                }
                Err(e) => return not_fetched(f, out, e),
            },
            Ok(_) => {
                *out = json!({"result": "notFetched"});
                return;
            }
            Err(e) => return not_fetched(f, out, e),
        },
    };
    let snap = match crate::change::snapshot(b, &commit) {
        Ok(s) => s,
        Err(e) => return not_fetched(f, out, e),
    };
    let Some(h) = snap.history.as_ref() else {
        return err(f, "FIXREF-06", format!("no history manifest at {commit}"));
    };
    let Some(rev) = h["revisions"]
        .as_array()
        .and_then(|a| a.iter().find(|r| r["id"] == p.revision.as_str()))
    else {
        return err(
            f,
            "FIXREF-06",
            format!("revision {} is not in history.yaml at {commit}", p.revision),
        );
    };
    if rev["status"] != "active" {
        // FIXREF-05: report retraction, merge or split; never forward silently.
        f.note(
            RF,
            "warning",
            "FIXREF-05",
            COMMON,
            format!(
                "revision {} is {}; replacedBy {}",
                p.revision, rev["status"], rev["replacedBy"]["revisions"]
            ),
            reference,
            "",
        );
    }
    let mut o = Map::new();
    o.insert("result".into(), json!("resolved"));
    o.insert("record".into(), json!(record));
    o.insert("revision".into(), json!(p.revision));
    o.insert("commit".into(), json!(commit));
    if let Some(node) = &p.node {
        let path = crate::repository::history::package_path(rev);
        let ast = match snap.asts.get(&p.revision) {
            Some(Ok(a)) => a,
            _ => {
                return err(
                    f,
                    "FIXREF-06",
                    format!("revision {} has no valid package at {commit}", p.revision),
                )
            }
        };
        let Some(text) = own_text(&ast["root"], node) else {
            return err(
                f,
                "FIXREF-07",
                format!("node {node} is not in revision {} at {commit}", p.revision),
            );
        };
        o.insert("path".into(), json!(path));
        o.insert("node".into(), json!(node));
        o.insert("text".into(), json!(text));
    }
    *out = Value::Object(o);
}

/// Own text of a node (children excluded), as in the semantic AST.
fn own_text(n: &Value, id: &str) -> Option<String> {
    if n["id"] == id {
        return Some(
            n["content"]
                .as_array()?
                .iter()
                .filter_map(|c| c["text"].as_str())
                .collect(),
        );
    }
    n["content"]
        .as_array()?
        .iter()
        .filter(|c| c.get("kind").is_some())
        .find_map(|c| own_text(c, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn string_forms() {
        let p = parse("loj:loj/a/b7k2m9qd@0123#a/main").unwrap();
        assert_eq!(p.commit.as_deref(), Some("0123"));
        assert_eq!(p.node.as_deref(), Some("a/main"));
        assert!(parse("loj:loj/a").is_err());
        assert!(parse("lo:loj/a/b").is_err());
        assert!(!full_hash("02086ca"));
        assert!(full_hash(&"a".repeat(64)));
    }

    /// A git failure after the bundle is open leaves the reference not fetched,
    /// with git's reason, and reports no error about the reference.
    #[test]
    fn git_failure_is_not_fetched_not_an_error() {
        let bundle = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../vendor/loj-spec/conformance/common/v1alpha1/valid/common-004/repo.bundle");
        let reference = "loj:loj/20990101-act-1/b7k2m9qd";
        let p = parse(reference).unwrap();
        let resolve = |b: &Bundle| {
            let mut f = Findings::default();
            let mut out = json!({"result": "error"});
            resolve_in(b, &p, "20990101-act-1", "main", reference, &mut f, &mut out);
            (f, out)
        };
        // The unbroken bundle resolves.
        let b = Bundle::open(&bundle).unwrap();
        let (f, out) = resolve(&b);
        assert_eq!(out["result"], "resolved", "{out} {:?}", f.items);
        // Without HEAD the directory is no repository: every git command fails.
        std::fs::remove_file(b.dir().join("HEAD")).unwrap();
        let (f, out) = resolve(&b);
        assert_eq!(out["result"], "notFetched", "{out}");
        assert!(!f.failed(RF), "{:?}", f.items);
        assert!(
            f.items
                .iter()
                .any(|(_, d)| d.class == "undetermined" && d.message.starts_with("git failed")),
            "{:?}",
            f.items
        );
    }
}
