//! Change check between two commits of a law repository (common/v1alpha1
//! CHG-02, CHG-03, CHG-04), reported apart from the conformance levels.
use crate::common::findings::Findings;
use crate::common::{load_yaml, Check, Class, Loaded, Report, COMMON};
use crate::git::{Bundle, ExportError, GitError};
use serde_json::{json, Map, Value};
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

pub(crate) fn snapshot(b: &Bundle, commit: &str) -> Result<Snapshot, ExportError> {
    let dir = b.export(commit)?;
    // The export holds every blob byte for byte, so the raw manifest is read from
    // it rather than from git again. No file of that name (absent, or a directory
    // in the tree) is no manifest, a finding about the data; failing to look at or
    // read the file leaves the check undetermined.
    let manifest = dir.join("history.yaml");
    let unreadable = |e: std::io::Error| {
        ExportError::Environment(GitError::Io(format!(
            "cannot read history.yaml at {commit}: {e}"
        )))
    };
    let history_bytes = match std::fs::metadata(&manifest) {
        Ok(m) if m.is_file() => Some(std::fs::read(&manifest).map_err(unreadable)?),
        Ok(_) => None,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(unreadable(e)),
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
fn undetermined(mut f: Findings, target: Map<String, Value>, why: String) -> Report {
    f.incomplete.insert(CH);
    f.note(CH, Class::Undetermined, "CHG-02", COMMON, why, "", "");
    let mut r = f.into_report(&[CH]);
    r.target = target;
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
                Class::Undetermined,
                "CHG-02",
                COMMON,
                e.to_string(),
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
/// finding about the data: it ends the check as incomplete with its reason
/// (VAL-04) at the first failure, so a broken git is not noted (or waited for)
/// once per commit. A tree refused for its content (an unsafe name, the input
/// budget) is noted for that commit and the check goes on.
fn check_bundle(b: &Bundle, mut f: Findings, mut report: Report, base: &str, head: &str) -> Report {
    let (base, head) = match (b.commit(base), b.commit(head)) {
        (Ok(Some(base)), Ok(Some(head))) => (base, head),
        (Err(e), _) | (_, Err(e)) => return undetermined(f, report.target, e.to_string()),
        _ => {
            let why = "base or head commit is not in the bundle".into();
            return undetermined(f, report.target, why);
        }
    };
    report.target.insert("commit".into(), json!(head));
    f.applicable.insert(CH);
    let commits = match b.range(&base, &head) {
        Ok(c) => c,
        Err(e) => return undetermined(f, report.target, e.to_string()),
    };
    // A refused tree is cached as `None`, so it is exported and noted only once
    // even when the commit is both a parent and a child in the range.
    let mut cache: BTreeMap<String, Option<Snapshot>> = BTreeMap::new();
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
                // Without a readable message the change kind is unknown, so the
                // commit's trees are not compared (as for a missing LoJ-Change).
                continue;
            }
            Err(e) => return undetermined(f, report.target, format!("{commit}: {e}")),
        };
        let Some((kind, declared)) = declared_change(&commit, &at, &message, &mut f) else {
            continue;
        };
        let Some(parent) = parent else {
            f.note(
                CH,
                Class::Undetermined,
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
            if let Err(why) = load_snapshot(b, c, &at, &mut cache, &mut f) {
                return undetermined(f, report.target, why);
            }
        }
        let (Some(Some(before)), Some(Some(after))) = (cache.get(&parent), cache.get(&commit))
        else {
            continue;
        };
        let change = Change {
            commit: &commit,
            at: &at,
            kind: &kind,
            declared: &declared,
        };
        change.compare(before, after, &mut f);
    }
    let mut r = f.into_report(&[CH]);
    r.target = report.target;
    r
}

/// CHG-03: the commit declares exactly one known `LoJ-Change`. Returns the
/// kind and the `LoJ-Revisions`, or `None` when the trees are not to be
/// compared because the kind is unknown.
fn declared_change(
    commit: &str,
    at: &str,
    message: &str,
    f: &mut Findings,
) -> Option<(String, Vec<String>)> {
    let (kinds, revisions) = trailers(message);
    let problem = match kinds.as_slice() {
        [k] if KINDS.contains(&k.as_str()) => {
            return Some((k.clone(), revisions.unwrap_or_default()))
        }
        [] => format!("{commit} declares no LoJ-Change"),
        [k] => format!("{commit}: unknown change kind {k}"),
        _ => format!("{commit} declares more than one LoJ-Change"),
    };
    f.error(CH, "CHG-03", COMMON, problem, at, "");
    None
}

/// Export `commit` unless the cache has it. A tree refused for its content is
/// noted and cached as `None`. `Err` is a failure of the environment, which
/// ends the whole check.
fn load_snapshot(
    b: &Bundle,
    commit: &str,
    at: &str,
    cache: &mut BTreeMap<String, Option<Snapshot>>,
    f: &mut Findings,
) -> Result<(), String> {
    if cache.contains_key(commit) {
        return Ok(());
    }
    match snapshot(b, commit) {
        Ok(s) => {
            cache.insert(commit.to_owned(), Some(s));
        }
        Err(ExportError::Environment(e)) => return Err(format!("{commit}: {e}")),
        Err(ExportError::Tree(e)) => {
            f.incomplete.insert(CH);
            f.note(
                CH,
                Class::Undetermined,
                "CHG-02",
                COMMON,
                format!("{commit}: tree refused: {e}"),
                at,
                "",
            );
            cache.insert(commit.to_owned(), None);
        }
    }
    Ok(())
}

/// One commit of the range and the change it declares.
struct Change<'a> {
    commit: &'a str,
    /// Location of its diagnostics.
    at: &'a str,
    /// `LoJ-Change`, one of [`KINDS`].
    kind: &'a str,
    /// `LoJ-Revisions`.
    declared: &'a [String],
}

impl Change<'_> {
    fn error(&self, f: &mut Findings, rule: &str, message: String) {
        f.error(CH, rule, COMMON, message, self.at, "");
    }

    /// CHG-02 / CHG-03 / CHG-04: compare the states before and after the commit.
    fn compare(&self, before: &Snapshot, after: &Snapshot, f: &mut Findings) {
        let (commit, kind) = (self.commit, self.kind);
        if matches!(
            kind,
            "correction" | "officialCorrection" | "restoration" | "revisionFix"
        ) && self.declared.is_empty()
        {
            self.error(
                f,
                "CHG-03",
                format!("{commit}: {kind} must declare LoJ-Revisions"),
            );
        }
        if kind == "migration" {
            f.incomplete.insert(CH);
            f.note(
                CH,
                Class::Undetermined,
                "CHG-02",
                COMMON,
                "migration needs the normative AST mapping of the new version",
                self.at,
                "",
            );
            return;
        }
        self.kept_asts(before, after, f);
        match kind {
            "format" if before.history_bytes != after.history_bytes => self.error(
                f,
                "CHG-02",
                format!("{commit}: format changes history.yaml"),
            ),
            "metadata" if before.packages != after.packages => self.error(
                f,
                "CHG-02",
                format!("{commit}: metadata changes the version packages"),
            ),
            "revisionAdd" if after.asts.keys().all(|k| before.asts.contains_key(k)) => self.error(
                f,
                "CHG-02",
                format!("{commit}: revisionAdd adds no version"),
            ),
            "revisionFix" => self.reused_ids(before, after, f),
            _ => {}
        }
    }

    /// Whether the AST of revision `id` must stay identical under this change.
    fn must_keep(&self, id: &str) -> bool {
        match self.kind {
            "format" | "metadata" | "revisionAdd" => true,
            _ => !self.declared.iter().any(|d| d == id),
        }
    }

    /// CHG-02: the ASTs that the change must keep are identical. CHG-04: an AST
    /// that cannot be built is never compared as equal.
    fn kept_asts(&self, before: &Snapshot, after: &Snapshot, f: &mut Findings) {
        let (commit, kind) = (self.commit, self.kind);
        for (id, old) in &before.asts {
            if !self.must_keep(id) {
                continue;
            }
            match (old, after.asts.get(id)) {
                (Ok(a), Some(Ok(b))) => {
                    if a != b {
                        self.error(
                            f,
                            "CHG-02",
                            format!("{commit}: {kind} changes the AST of revision {id}"),
                        );
                    }
                }
                (_, None) => {
                    if kind != "revisionFix" || !self.declared.contains(id) {
                        self.error(
                            f,
                            "CHG-02",
                            format!("{commit}: {kind} removes revision {id}"),
                        );
                    }
                }
                (Err(e), _) | (_, Some(Err(e))) => {
                    f.incomplete.insert(CH);
                    f.note(
                        CH,
                        Class::Undetermined,
                        "CHG-04",
                        COMMON,
                        format!("{commit}: revision {id} not compared: {e}"),
                        self.at,
                        "",
                    );
                }
            }
        }
    }

    /// CHG-02 (revisionFix): retracted, merged or split IDs are not reused.
    fn reused_ids(&self, before: &Snapshot, after: &Snapshot, f: &mut Findings) {
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
                self.error(
                    f,
                    "CHG-02",
                    format!("{}: revision ID {id} is reused", self.commit),
                );
            }
        }
    }
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
    fn a_commit_declares_exactly_one_known_change() {
        let mut f = Findings::default();
        let declared =
            |message: &str, f: &mut Findings| declared_change("c1", "commit c1", message, f);
        assert_eq!(
            declared(
                "Fix\n\nLoJ-Change: correction\nLoJ-Revisions: r1, r2\n",
                &mut f
            ),
            Some((
                "correction".to_owned(),
                vec!["r1".to_owned(), "r2".to_owned()]
            ))
        );
        assert_eq!(
            declared("Tidy\n\nLoJ-Change: format", &mut f),
            Some(("format".to_owned(), Vec::new()))
        );
        assert!(f.items.is_empty());
        for (message, problem) in [
            ("No trailer\n", "c1 declares no LoJ-Change"),
            // Only the last paragraph holds trailers.
            ("LoJ-Change: format\n\nBody\n", "c1 declares no LoJ-Change"),
            (
                "X\n\nLoJ-Change: rewrite\n",
                "c1: unknown change kind rewrite",
            ),
            (
                "X\n\nLoJ-Change: format\nLoJ-Change: metadata\n",
                "c1 declares more than one LoJ-Change",
            ),
        ] {
            let mut f = Findings::default();
            assert_eq!(declared(message, &mut f), None, "{message:?}");
            let found: Vec<_> = f
                .items
                .iter()
                .map(|(_, d)| (d.rule.as_str(), d.message.as_str()))
                .collect();
            assert_eq!(found, [("CHG-03", problem)]);
        }
    }

    #[test]
    fn which_asts_a_change_must_keep() {
        let declared = ["r2".to_owned()];
        let change = |kind| Change {
            commit: "c1",
            at: "commit c1",
            kind,
            declared: &declared,
        };
        // These kinds never change an AST, whatever they declare.
        for kind in ["format", "metadata", "revisionAdd"] {
            assert!(change(kind).must_keep("r1"), "{kind}");
            assert!(change(kind).must_keep("r2"), "{kind}");
        }
        // The others may change the declared revisions only.
        for kind in [
            "correction",
            "officialCorrection",
            "restoration",
            "revisionFix",
        ] {
            assert!(change(kind).must_keep("r1"), "{kind}");
            assert!(!change(kind).must_keep("r2"), "{kind}");
        }
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
    /// and reports no finding about the data.
    #[test]
    fn git_failure_is_incomplete_not_a_finding() {
        let s = Scratch::new();
        let (path, c) = s.chain(&[b"base\n", b"head\n\nLoJ-Change: metadata\n"]);
        let b = Bundle::open(&path).unwrap();
        b.break_for_test();
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
                .any(|d| d.class == Class::Undetermined && d.message.starts_with("git failed")),
            "{:?}",
            r.diagnostics
        );
    }

    /// A refused tree is exported and noted once, even when its commit is both a
    /// child and a parent in the range.
    #[test]
    fn refused_tree_is_noted_once() {
        let s = Scratch::new();
        let blob = s.run(&["hash-object", "-w", "--stdin"], Some(b""));
        let tree = |name: &str| {
            let spec = format!("100644 blob {blob}\t{name}\n");
            s.run(&["mktree"], Some(spec.as_bytes()))
        };
        let (good, bad) = (tree("history.yaml"), tree("a:b"));
        let msg = "m\n\nLoJ-Change: metadata\n";
        let base = s.run(&["commit-tree", &good, "-m", msg], None);
        let mid = s.run(&["commit-tree", &bad, "-p", &base, "-m", msg], None);
        let head = s.run(&["commit-tree", &good, "-p", &mid, "-m", msg], None);
        let r = check(&s.bundle_head(&head), &base, &head);
        assert_eq!(result(&r), ResultValue::Incomplete, "{:?}", r.diagnostics);
        let notes = r
            .diagnostics
            .iter()
            .filter(|d| d.message.starts_with(&format!("{mid}:")))
            .count();
        assert_eq!(notes, 1, "{:?}", r.diagnostics);
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
