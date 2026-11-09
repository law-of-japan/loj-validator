//! Change check between two commits of a law repository (common/v1alpha1
//! CHG-02, CHG-03, CHG-04), reported apart from the conformance levels.
use crate::common::findings::Findings;
use crate::common::{load_yaml, Check, Loaded, Report, COMMON};
use crate::git::Bundle;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

const CH: Check = Check::CHANGE;
const KINDS: &[&str] = &[
    "format",
    "correction",
    "officialCorrection",
    "restoration",
    "metadata",
    "revisionAdd",
    "revisionFix",
    "migration",
];

/// The state of a record version at one commit.
pub(crate) struct Snapshot {
    pub history: Option<Value>,
    pub history_bytes: Option<Vec<u8>>,
    /// revision_id -> AST, or why no AST could be built (CHG-04).
    pub asts: BTreeMap<String, Result<Value, String>>,
    /// revision_id -> package path.
    pub packages: BTreeMap<String, String>,
}

pub(crate) fn snapshot(b: &Bundle, commit: &str) -> Result<Snapshot, String> {
    let dir = b.export(commit)?;
    // The export holds every blob byte for byte, so the raw manifest is read from
    // it rather than from git again. No file of that name (absent, or a directory
    // in the tree) is no manifest, a finding about the data; only a failure to read
    // a file that is there leaves the check undetermined.
    let manifest = dir.join("history.yaml");
    let history_bytes = match std::fs::read(&manifest) {
        Ok(bytes) => Some(bytes),
        Err(_) if !manifest.is_file() => None,
        Err(e) => return Err(format!("cannot read history.yaml at {commit}: {e}")),
    };
    let history = match load_yaml(&dir.join("history.yaml")) {
        Loaded::Ok(v, _) => Some(v),
        _ => None,
    };
    let mut asts = BTreeMap::new();
    let mut packages = BTreeMap::new();
    for r in history
        .as_ref()
        .and_then(|h| h["revisions"].as_array())
        .into_iter()
        .flatten()
    {
        let Some(id) = r["id"].as_str() else { continue };
        if !matches!(r["text"]["status"].as_str(), Some("complete" | "partial")) {
            continue;
        }
        let path = crate::repository::history::package_path(r);
        let pkg = crate::v1alpha2::check(&dir.join(&path), &[Check::L0, Check::L1]);
        let ast = match pkg.ast {
            Some(a) => Ok(a),
            None if r["text"]["status"] == "partial" => Err("partial text".into()),
            None => Err("the package does not pass L1".into()),
        };
        asts.insert(id.to_owned(), ast);
        packages.insert(id.to_owned(), path);
    }
    Ok(Snapshot {
        history,
        history_bytes,
        asts,
        packages,
    })
}

/// `LoJ-Change` and `LoJ-Revisions` trailers of the last paragraph.
fn trailers(message: &str) -> (Vec<String>, Option<Vec<String>>) {
    let last = message
        .trim_end()
        .rsplit("\n\n")
        .next()
        .unwrap_or("")
        .to_owned();
    let mut kinds = Vec::new();
    let mut revisions = None;
    for line in last.lines() {
        if let Some(v) = line.strip_prefix("LoJ-Change:") {
            kinds.push(v.trim().to_owned());
        } else if let Some(v) = line.strip_prefix("LoJ-Revisions:") {
            revisions = Some(v.trim().split(", ").map(str::to_owned).collect());
        }
    }
    (kinds, revisions)
}

/// End the check as incomplete (VAL-04), with `why` as the reason. The note
/// carries CHG-02, the rule of the change check as a whole, whichever step (commit
/// resolution, range, message) could not be completed: the spec gives no rule of
/// its own for an undetermined change check.
fn undetermined(mut f: Findings, report: Report, why: String) -> Report {
    f.incomplete.insert(CH);
    f.note(CH, "undetermined", "CHG-02", COMMON, why, "", "");
    let mut r = f.into_report(&[CH]);
    r.target = report.target;
    r
}

pub fn check(bundle: &Path, base: &str, head: &str) -> Report {
    let mut f = Findings::default();
    let mut report = Report::default();
    let b = match Bundle::open(bundle) {
        Ok(b) => b,
        Err(e) => {
            f.incomplete.insert(CH);
            f.note(
                CH,
                "undetermined",
                "CHG-02",
                COMMON,
                e,
                &bundle.to_string_lossy(),
                "",
            );
            let mut r = f.into_report(&[CH]);
            r.target
                .insert("repository".into(), json!(bundle.to_string_lossy()));
            return r;
        }
    };
    report
        .target
        .insert("repository".into(), json!(bundle.to_string_lossy()));
    check_bundle(&b, f, report, base, head)
}

/// The change check on an opened bundle. A git failure is never reported as a
/// finding about the data. A failure to resolve the commits, list the range or
/// read a message ends the check as incomplete with its reason (VAL-04), so a
/// broken git is not noted once per commit. A tree that cannot be exported is
/// noted for that commit and the check goes on: the cause may lie in that tree
/// (an unsafe name, the input budget) rather than in git.
fn check_bundle(b: &Bundle, mut f: Findings, mut report: Report, base: &str, head: &str) -> Report {
    let (base, head) = match (b.commit(base), b.commit(head)) {
        (Ok(Some(base)), Ok(Some(head))) => (base, head),
        (Err(e), _) | (_, Err(e)) => return undetermined(f, report, e),
        _ => {
            let why = "base or head commit is not in the bundle".into();
            return undetermined(f, report, why);
        }
    };
    report.target.insert("commit".into(), json!(head));
    f.applicable.insert(CH);
    let commits = match b.range(&base, &head) {
        Ok(c) => c,
        Err(e) => return undetermined(f, report, e),
    };
    let mut cache: BTreeMap<String, Snapshot> = BTreeMap::new();
    for (commit, parent) in commits {
        let at = format!("commit {commit}");
        let message = match b.message(&commit) {
            Ok(Some(m)) => m,
            Ok(None) => {
                f.error(
                    CH,
                    "CHG-03",
                    COMMON,
                    format!("{commit}: the commit message is not UTF-8"),
                    &at,
                    "",
                );
                continue;
            }
            Err(e) => return undetermined(f, report, format!("{commit}: {e}")),
        };
        let (kinds, revisions) = trailers(&message);
        let kind = match kinds.as_slice() {
            [k] if KINDS.contains(&k.as_str()) => k.clone(),
            [] => {
                f.error(
                    CH,
                    "CHG-03",
                    COMMON,
                    format!("{commit} declares no LoJ-Change"),
                    &at,
                    "",
                );
                continue;
            }
            [k] => {
                f.error(
                    CH,
                    "CHG-03",
                    COMMON,
                    format!("{commit}: unknown change kind {k}"),
                    &at,
                    "",
                );
                continue;
            }
            _ => {
                f.error(
                    CH,
                    "CHG-03",
                    COMMON,
                    format!("{commit} declares more than one LoJ-Change"),
                    &at,
                    "",
                );
                continue;
            }
        };
        let declared = revisions.unwrap_or_default();
        let Some(parent) = parent else {
            f.note(
                CH,
                "undetermined",
                "CHG-02",
                COMMON,
                format!("{commit} has no parent to compare"),
                &at,
                "",
            );
            f.incomplete.insert(CH);
            continue;
        };
        for c in [&parent, &commit] {
            if !cache.contains_key(c.as_str()) {
                match snapshot(b, c) {
                    Ok(s) => {
                        cache.insert(c.clone(), s);
                    }
                    Err(e) => {
                        f.incomplete.insert(CH);
                        f.note(CH, "undetermined", "CHG-02", COMMON, e, &at, "");
                    }
                }
            }
        }
        let (Some(before), Some(after)) = (cache.get(&parent), cache.get(&commit)) else {
            continue;
        };
        if matches!(
            kind.as_str(),
            "correction" | "officialCorrection" | "restoration" | "revisionFix"
        ) && declared.is_empty()
        {
            f.error(
                CH,
                "CHG-03",
                COMMON,
                format!("{commit}: {kind} must declare LoJ-Revisions"),
                &at,
                "",
            );
        }
        if kind == "migration" {
            f.incomplete.insert(CH);
            f.note(
                CH,
                "undetermined",
                "CHG-02",
                COMMON,
                "migration needs the normative AST mapping of the new version",
                &at,
                "",
            );
            continue;
        }
        // Versions whose AST must stay identical.
        let must_keep = |id: &str| match kind.as_str() {
            "format" | "metadata" | "revisionAdd" => true,
            _ => !declared.iter().any(|d| d == id),
        };
        for (id, old) in &before.asts {
            if !must_keep(id) {
                continue;
            }
            match (old, after.asts.get(id)) {
                (Ok(a), Some(Ok(b))) => {
                    if a != b {
                        f.error(
                            CH,
                            "CHG-02",
                            COMMON,
                            format!("{commit}: {kind} changes the AST of revision {id}"),
                            &at,
                            "",
                        );
                    }
                }
                (_, None) => {
                    if kind != "revisionFix" || !declared.contains(id) {
                        f.error(
                            CH,
                            "CHG-02",
                            COMMON,
                            format!("{commit}: {kind} removes revision {id}"),
                            &at,
                            "",
                        );
                    }
                }
                (Err(e), _) | (_, Some(Err(e))) => {
                    // CHG-04: an AST that cannot be built is never compared as equal.
                    f.incomplete.insert(CH);
                    f.note(
                        CH,
                        "undetermined",
                        "CHG-04",
                        COMMON,
                        format!("{commit}: revision {id} not compared: {e}"),
                        &at,
                        "",
                    );
                }
            }
        }
        if kind == "format" && before.history_bytes != after.history_bytes {
            f.error(
                CH,
                "CHG-02",
                COMMON,
                format!("{commit}: format changes history.yaml"),
                &at,
                "",
            );
        }
        if matches!(kind.as_str(), "metadata") && before.packages != after.packages {
            f.error(
                CH,
                "CHG-02",
                COMMON,
                format!("{commit}: metadata changes the version packages"),
                &at,
                "",
            );
        }
        if kind == "revisionAdd" {
            let added: Vec<_> = after
                .asts
                .keys()
                .filter(|k| !before.asts.contains_key(*k))
                .collect();
            if added.is_empty() {
                f.error(
                    CH,
                    "CHG-02",
                    COMMON,
                    format!("{commit}: revisionAdd adds no version"),
                    &at,
                    "",
                );
            }
        }
        if kind == "revisionFix" {
            // Retracted, merged or split IDs are not reused.
            let status = |s: &Snapshot, id: &str| {
                s.history.as_ref().and_then(|h| {
                    h["revisions"]
                        .as_array()?
                        .iter()
                        .find(|r| r["id"] == id)
                        .map(|r| r["status"].clone())
                })
            };
            for id in before.asts.keys() {
                if status(before, id).is_some_and(|s| s != "active")
                    && status(after, id).is_some_and(|s| s == "active")
                {
                    f.error(
                        CH,
                        "CHG-02",
                        COMMON,
                        format!("{commit}: revision ID {id} is reused"),
                        &at,
                        "",
                    );
                }
            }
        }
    }
    let mut r = f.into_report(&[CH]);
    r.target = report.target;
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::ResultValue;
    use crate::git::tests::Scratch;

    fn result(r: &Report) -> ResultValue {
        r.results.iter().find(|(c, _)| *c == CH).unwrap().1
    }

    #[test]
    fn non_utf8_message_is_a_chg03_error() {
        let s = Scratch::new();
        let (path, c) = s.chain(&[b"base\n", b"bad \xff\xfe\n"]);
        let r = check(&path, &c[0], &c[1]);
        assert_eq!(result(&r), ResultValue::Fail);
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.rule == "CHG-03" && d.message.contains("not UTF-8")),
            "{:?}",
            r.diagnostics
        );
    }

    /// When git fails (not "absent"), the check is incomplete with git's reason,
    /// and reports no finding about the data. Without `HEAD` the directory is no
    /// repository, so every git command exits with status 128.
    #[test]
    fn git_failure_is_incomplete_not_a_finding() {
        let s = Scratch::new();
        let (path, c) = s.chain(&[b"base\n", b"head\n\nLoJ-Change: metadata\n"]);
        let b = Bundle::open(&path).unwrap();
        std::fs::remove_file(b.dir().join("HEAD")).unwrap();
        let r = check_bundle(&b, Findings::default(), Report::default(), &c[0], &c[1]);
        assert_eq!(result(&r), ResultValue::Incomplete);
        assert!(
            r.diagnostics.iter().all(|d| !d.is_error()),
            "{:?}",
            r.diagnostics
        );
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.class == "undetermined" && d.message.starts_with("git failed")),
            "{:?}",
            r.diagnostics
        );
    }

    /// A directory named `history.yaml` is no manifest, not an unreadable one.
    #[test]
    fn history_directory_is_no_manifest() {
        let s = Scratch::new();
        let blob = s.run(&["hash-object", "-w", "--stdin"], Some(b"x\n"));
        let inner = format!("100644 blob {blob}\tx\n");
        let inner = s.run(&["mktree"], Some(inner.as_bytes()));
        let outer = format!("040000 tree {inner}\thistory.yaml\n");
        let outer = s.run(&["mktree"], Some(outer.as_bytes()));
        let path = s.bundle_tree(&outer);
        let b = Bundle::open(&path).unwrap();
        let head = b.branch("main").unwrap().unwrap();
        let snap = snapshot(&b, &head).unwrap();
        assert!(snap.history.is_none() && snap.history_bytes.is_none());
    }
}
