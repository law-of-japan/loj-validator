//! Change check between two commits of a law repository (common/v1alpha1
//! CHG-02, CHG-03, CHG-04), reported apart from the conformance levels.
use crate::checks::RepositoryKind;
use crate::common::findings::Findings;
use crate::common::{Check, Class, Report, COMMON};
use crate::git::{Bundle, GitError};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{Duration, Instant};

const CH: Check = Check::CHANGE;
mod policy;
mod snapshots;
use policy::{declared_change, reviewed};
pub(crate) use snapshots::snapshot;
use snapshots::{load_snapshot, Snapshot};

/// Implementation limits of one change check (VAL-04): past either, the check
/// ends as incomplete rather than running for as long as the input allows.
struct Limits {
    /// The most commits checked.
    commits: usize,
    /// The longest the check runs, measured before each commit: the commit
    /// being checked when it passes finishes first (each git command has its
    /// own timeout).
    duration: Duration,
    /// The time since the check started (a parameter so tests can let the
    /// limit pass between two commits).
    elapsed: fn(Instant) -> Duration,
}

const LIMITS: Limits = Limits {
    commits: 10_000,
    duration: Duration::from_secs(3600),
    elapsed: |start| start.elapsed(),
};
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

/// The change check (CHG-02, CHG-03) of the commits after `base` up to `head`,
/// or, without `base`, of every commit reachable from `head` from the first
/// commit on; a commit without a parent is compared with no record before it.
#[cfg(test)]
fn check(bundle: &Path, base: Option<&str>, head: &str) -> Report {
    check_repository(bundle, base, head, RepositoryKind::Law)
}

/// Check commits using the explicitly selected repository layout.
pub fn check_repository(
    bundle: &Path,
    base: Option<&str>,
    head: &str,
    repository: RepositoryKind,
) -> Report {
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
    check_bundle(&b, f, report, base, head, repository, &LIMITS)
}

/// The change check on an opened bundle. A git failure is never reported as a
/// finding about the data: it ends the check as incomplete with its reason
/// (VAL-04) at the first failure, so a broken git is not noted (or waited for)
/// once per commit. A tree refused for its content (an unsafe name, the input
/// budget) is noted for that commit and the check goes on.
fn check_bundle(
    b: &Bundle,
    mut f: Findings,
    mut report: Report,
    base: Option<&str>,
    head: &str,
    repository: RepositoryKind,
    limits: &Limits,
) -> Report {
    let start = Instant::now();
    let base = base.map(|base| b.commit(base)).transpose();
    let (base, head) = match (base, b.commit(head)) {
        (Ok(base @ (None | Some(Some(_)))), Ok(Some(head))) => (base.flatten(), head),
        (Err(e), _) | (_, Err(e)) => return undetermined(f, report.target, e.to_string()),
        _ => {
            let why = "base or head commit is not in the bundle".into();
            return undetermined(f, report.target, why);
        }
    };
    report.target.insert("commit".into(), json!(head));
    f.applicable.insert(CH);
    let commits = match b.range(base.as_deref(), &head, limits.commits) {
        Ok(Some(c)) => c,
        // Past the limit, no commit is checked: a check of only some commits
        // would say nothing about the rest.
        Ok(None) => {
            let why = format!("more than {} commits to check", limits.commits);
            return undetermined(f, report.target, why);
        }
        Err(e) => return undetermined(f, report.target, e.to_string()),
    };
    // A refused tree is cached as `None`, so it is exported and noted only once
    // even when the commit is both a parent and a child in the range.
    let mut cache: BTreeMap<String, Option<Snapshot>> = BTreeMap::new();
    // Commits whose unusable history manifest has been noted, once each.
    let mut noted: BTreeSet<String> = BTreeSet::new();
    // How many commits of the list compare with each commit as their parent:
    // a snapshot is dropped from the cache after its last such comparison, so
    // a check of the whole history does not keep every snapshot in memory.
    let mut uses: BTreeMap<String, usize> = BTreeMap::new();
    for (_, parents) in &commits {
        if let [parent] = parents.as_slice() {
            *uses.entry(parent.clone()).or_default() += 1;
        }
    }
    for (commit, parents) in commits {
        if crate::cancellation::checkpoint() {
            return undetermined(f, report.target, GitError::Cancelled.to_string());
        }
        if (limits.elapsed)(start) >= limits.duration {
            let why = format!(
                "the change check ran longer than {} s",
                limits.duration.as_secs()
            );
            return undetermined(f, report.target, why);
        }
        let last_use = match parents.as_slice() {
            [parent] => uses.get_mut(parent).is_some_and(|n| {
                *n -= 1;
                *n == 0
            }),
            _ => false,
        };
        'commit: {
            let at = format!("commit {commit}");
            if parents.len() > 1 {
                f.error(
                    CH,
                    "CHG-03",
                    COMMON,
                    "merge commits are not permitted",
                    &at,
                    "",
                );
                break 'commit;
            }
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
                    break 'commit;
                }
                Err(e) => return undetermined(f, report.target, format!("{commit}: {e}")),
            };
            let Some((kind, declared)) = declared_change(&commit, &at, &message, &mut f) else {
                break 'commit;
            };
            let parent = parents.first();
            let (layout, first_kind) = match repository {
                RepositoryKind::Law => ("law", "revisionAdd"),
                RepositoryKind::Registry => ("registry", "metadata"),
            };
            if parent.is_none() && kind != first_kind {
                f.error(
                    CH,
                    "CHG-03",
                    COMMON,
                    format!("first {layout} commit must declare {first_kind}"),
                    &at,
                    "",
                );
                break 'commit;
            }
            let change = Change {
                initial: parent.is_none(),
                commit: &commit,
                at: &at,
                kind: &kind,
                declared: &declared,
                reviewed: reviewed(&message),
            };
            for c in parent.into_iter().chain(std::iter::once(&commit)) {
                if let Err(why) = load_snapshot(b, c, repository, &change, &mut cache, &mut f) {
                    return undetermined(f, report.target, why);
                }
            }
            let empty = Snapshot::default();
            let before = match parent {
                Some(parent) => match cache.get(parent) {
                    Some(Some(snapshot)) => snapshot,
                    _ => break 'commit,
                },
                None => &empty,
            };
            let Some(Some(after)) = cache.get(&commit) else {
                break 'commit;
            };
            // CHG-02: the file modes are read from the trees, so they are checked
            // for every declared kind even when a history manifest cannot be used.
            // `auxiliary` fails any mode change of canonical data in its own check.
            if kind != "auxiliary" {
                policy::modes(&change, before, after, &mut f);
            }
            // CHG-02 `auxiliary` compares the files only, so it is decided even
            // when a history manifest cannot be used; every other kind reads it.
            // `migration` stays undetermined here too, as when such a tree was
            // refused before, rather than reaching its CHG-02 error in `compare`.
            if kind != "auxiliary" {
                let unusable: Vec<_> = [parent.map(|p| (p, before)), Some((&commit, after))]
                    .into_iter()
                    .flatten()
                    .filter_map(|(c, s)| Some((c, s.manifest_error.as_ref()?)))
                    .collect();
                for (c, e) in &unusable {
                    if noted.insert((*c).clone()) {
                        f.incomplete.insert(CH);
                        let why = format!("{c}: tree refused: {e}");
                        f.note(CH, Class::Undetermined, "CHG-02", COMMON, why, &at, "");
                    }
                }
                if !unusable.is_empty() {
                    break 'commit;
                }
            }
            change.compare(before, after, &mut f);
        }
        if last_use {
            cache.remove(&parents[0]);
        }
        // A commit that no later commit compares with (the head, or one whose
        // child is a merge commit) is not needed again either.
        if !uses.contains_key(&commit) {
            cache.remove(&commit);
        }
    }
    let mut r = f.into_report(&[CH]);
    r.target = report.target;
    r
}

/// One commit of the range and the change it declares.
struct Change<'a> {
    initial: bool,
    commit: &'a str,
    /// Location of its diagnostics.
    at: &'a str,
    /// `LoJ-Change`, one of the supported change kinds.
    kind: &'a str,
    /// `LoJ-Revisions`.
    declared: &'a [String],
    reviewed: bool,
}

#[cfg(test)]
mod tests {
    use super::policy::{trailers, KINDS};
    use super::snapshots::read_snapshot;
    use super::*;
    use crate::common::ResultValue;
    use crate::git::tests::Scratch;
    use crate::git::ExportError;

    fn result(r: &Report) -> ResultValue {
        r.results.iter().find(|(c, _)| *c == CH).unwrap().1
    }

    #[test]
    fn trailer_grammar_is_independent_of_git_and_unicode_whitespace() {
        for text in [
            "Title\r\n \t\r\nLoJ-Change:\tformat \t\r\n\r\n",
            "Title\n\nLoJ-Change:format",
        ] {
            assert_eq!(trailers(text).0, ["format"]);
        }
        for text in [
            "LoJ-Change: format",
            "Title\n\nLoJ-Change : format",
            "Title\n\nLoJ-Change: format\nnot-a-trailer",
            "Title\n\nLoJ-Change: format\n\nBody",
            "Title\r\rLoJ-Change: format",
            "Title\n　\nLoJ-Change: format",
            "Title\n\nloj-change: format",
        ] {
            assert!(trailers(text).0.is_empty(), "{text:?}");
        }
        assert_eq!(
            trailers("Title\n\nLoJ-Change:　format\u{a0}").0,
            ["　format\u{a0}"]
        );
        assert!(reviewed("Title\n\nLoJ-Reviewed: person"));
        assert!(reviewed("Title\n\nLoJ-Reviewed:　"));
        for text in [
            "Title\n\nLoJ-Reviewed:",
            "Title\n\nLoJ-Reviewed: \t",
            "Title\n\nLoJ-Reviewed: a\nLoJ-Reviewed: b",
        ] {
            assert!(!reviewed(text));
        }
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
    fn duplicate_revision_trailers_are_rejected_before_comparison() {
        for kind in KINDS {
            for (first, second) in [("r1", "r1"), ("r1", "r2"), ("", "r1"), ("", "")] {
                let message = format!(
                    "Change\n\nLoJ-Change: {kind}\nLoJ-Revisions: {first}\nLoJ-Revisions: {second}\n"
                );
                let mut f = Findings::default();
                assert_eq!(declared_change("c1", "commit c1", &message, &mut f), None);
                assert_eq!(f.items.len(), 1);
                assert_eq!(f.items[0].1.rule, "CHG-03");
                assert_eq!(f.items[0].1.class, Class::Error);
            }
        }
        // These trees have an unreadable history manifest. Exporting them would
        // add an undetermined finding; reject the trailers before loading them.
        let s = Scratch::new();
        let (path, commits) = s.chain(&[
            b"base\n",
            b"Change\n\nLoJ-Change: migration\nLoJ-Revisions:\nLoJ-Revisions:\n",
        ]);
        let report = check(&path, Some(&commits[0]), &commits[1]);
        assert_eq!(result(&report), ResultValue::Fail);
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].rule, "CHG-03");
    }

    #[test]
    fn cached_snapshots_build_asts_only_when_a_comparison_needs_them() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/loj-spec");
        let manifest: Value = serde_json::from_slice(
            &std::fs::read(root.join("conformance/common/v1alpha1/manifest.json")).unwrap(),
        )
        .unwrap();
        // These case IDs belong to the pinned specification; revisiting the pin
        // also requires preserving these complete, invalid-L1, and partial inputs.
        for case_id in ["common-041", "common-048", "common-049"] {
            let case = manifest["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|case| case["id"] == case_id)
                .unwrap();
            let b = Bundle::open(&root.join(case["input"]["bundle"].as_str().unwrap())).unwrap();
            let commit = case["input"]["base"].as_str().unwrap();
            let declared = ["b7k2m9qd".to_owned()];
            for kind in ["correction", "officialCorrection", "restoration"] {
                let mut change = Change {
                    initial: false,
                    commit,
                    at: "test commit",
                    kind,
                    declared: &declared,
                    reviewed: false,
                };
                let mut cache = BTreeMap::new();
                let mut f = Findings::default();
                load_snapshot(&b, commit, RepositoryKind::Law, &change, &mut cache, &mut f)
                    .unwrap();
                let cached = cache[commit].as_ref().unwrap();
                assert!(cached.asts.is_empty());
                assert!(cached.packages.contains_key(&declared[0]));
                let directory = cached.directory.clone();

                // The next commit requires all ASTs, even if it declares IDs.
                change.kind = "metadata";
                load_snapshot(&b, commit, RepositoryKind::Law, &change, &mut cache, &mut f)
                    .unwrap();
                let cached = cache[commit].as_ref().unwrap();
                assert_eq!(cached.directory, directory);
                assert_eq!(cached.asts.len(), 1);
                assert_eq!(cached.asts, snapshot(&b, commit).unwrap().asts);
                assert!(f.items.is_empty());

                // Undeclared versions must still be checked during correction.
                change.kind = kind;
                change.declared = &[];
                let mut other_cache = BTreeMap::new();
                load_snapshot(
                    &b,
                    commit,
                    RepositoryKind::Law,
                    &change,
                    &mut other_cache,
                    &mut f,
                )
                .unwrap();
                assert_eq!(other_cache[commit].as_ref().unwrap().asts, cached.asts);
                change.compare(cached, cached, &mut f);
                if case_id != "common-041" {
                    assert!(f
                        .items
                        .iter()
                        .any(|(_, d)| d.rule == "CHG-04" && d.class == Class::Undetermined));
                }
            }
        }
    }

    #[test]
    fn which_asts_a_change_must_keep() {
        let declared = ["r2".to_owned()];
        let change = |kind| Change {
            initial: false,
            commit: "c1",
            at: "commit c1",
            kind,
            declared: &declared,
            reviewed: false,
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
        let r = check(&path, Some(&c[0]), &c[1]);
        assert_eq!(result(&r), ResultValue::Fail);
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.rule == "CHG-03" && d.message.contains("not UTF-8")),
            "{:?}",
            r.diagnostics
        );
    }

    /// Cancellation observed while walking the exported files is reported as a
    /// cancellation, not as a git or file-system failure.
    #[test]
    fn snapshot_cancellation_is_its_own_kind() {
        let s = Scratch::new();
        let b = Bundle::open(&s.bundle(&[("100644", "blob", "law.yaml")])).unwrap();
        let head = b.branch("main").unwrap().unwrap();
        let (whole, checkpoints) =
            crate::cancellation::test_hook::run(None, || read_snapshot(&b, &head, None).is_ok());
        assert!(whole);
        assert!(checkpoints > 0, "the walk has checkpoints");
        for at in 1..=checkpoints {
            let (result, _) =
                crate::cancellation::test_hook::run(Some(at), || read_snapshot(&b, &head, None));
            assert!(
                matches!(result, Err(ExportError::Cancelled)),
                "checkpoint {at}: {:?}",
                result.err()
            );
        }
    }

    /// When git fails (not "absent"), the check is incomplete with git's reason,
    /// and reports no finding about the data.
    #[test]
    fn git_failure_is_incomplete_not_a_finding() {
        let s = Scratch::new();
        let (path, c) = s.chain(&[b"base\n", b"head\n\nLoJ-Change: metadata\n"]);
        let b = Bundle::open(&path).unwrap();
        b.break_for_test();
        let r = check_bundle(
            &b,
            Findings::default(),
            Report::default(),
            Some(&c[0]),
            &c[1],
            RepositoryKind::Law,
            &LIMITS,
        );
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
        let r = check(&s.bundle_head(&head), Some(&base), &head);
        assert_eq!(result(&r), ResultValue::Incomplete, "{:?}", r.diagnostics);
        let notes = r
            .diagnostics
            .iter()
            .filter(|d| d.message.starts_with(&format!("{mid}:")))
            .count();
        assert_eq!(notes, 1, "{:?}", r.diagnostics);
    }

    /// CHG-02 `auxiliary`: the Git file mode reaches the snapshot from the tree,
    /// so an executable bit on canonical data fails and one outside it passes.
    #[test]
    fn auxiliary_compares_file_modes_read_from_the_bundle() {
        let s = Scratch::new();
        let blob = s.run(&["hash-object", "-w", "--stdin"], Some(b"x\n"));
        let tree = |rebuild: &str, generator: &str| {
            let spec =
                format!("{rebuild} blob {blob}\trebuild.yaml\n{generator} blob {blob}\tgen.sh\n");
            s.run(&["mktree"], Some(spec.as_bytes()))
        };
        let msg = "m\n\nLoJ-Change: auxiliary\n";
        let base = s.run(&["commit-tree", &tree("100644", "100644"), "-m", msg], None);
        for (rebuild, generator, expected) in [
            ("100644", "100755", ResultValue::Pass),
            ("100755", "100644", ResultValue::Fail),
        ] {
            let head = s.run(
                &[
                    "commit-tree",
                    &tree(rebuild, generator),
                    "-p",
                    &base,
                    "-m",
                    msg,
                ],
                None,
            );
            let r = check(&s.bundle_head(&head), Some(&base), &head);
            assert_eq!(result(&r), expected, "{:?}", r.diagnostics);
            if expected == ResultValue::Fail {
                assert!(
                    r.diagnostics.iter().any(|d| d.rule == "CHG-02"
                        && d.message
                            .contains("file mode of canonical data file rebuild.yaml")),
                    "{:?}",
                    r.diagnostics
                );
            }
        }
    }

    /// CHG-02: the file mode is read from the tree, so a canonical data file
    /// made executable fails every kind, even when the history manifest cannot
    /// be used and the rest of the commit stays undetermined.
    #[test]
    fn file_modes_fail_without_a_usable_history_manifest() {
        let s = Scratch::new();
        let blob = s.run(&["hash-object", "-w", "--stdin"], Some(b"x\n"));
        let history = s.run(&["hash-object", "-w", "--stdin"], Some(b"revisions: [\n"));
        let tree = |main: &str, generator: &str| {
            let spec =
                format!("100644 blob {history}\thistory.yaml\n{generator} blob {blob}\tgen.sh\n");
            let package = format!("{main} blob {blob}\tmain.md\n");
            let package = s.run(&["mktree"], Some(package.as_bytes()));
            let version = format!("040000 tree {package}\tb7k2m9qd\n");
            let version = s.run(&["mktree"], Some(version.as_bytes()));
            let spec = format!("{spec}040000 tree {version}\tversions\n");
            s.run(&["mktree"], Some(spec.as_bytes()))
        };
        let base = s.run(
            &["commit-tree", &tree("100644", "100644"), "-m", "base"],
            None,
        );
        for kind in ["format", "metadata", "correction", "migration"] {
            let msg = format!("m\n\nLoJ-Change: {kind}\nLoJ-Revisions: b7k2m9qd\n");
            let commit = |tree: &str| s.run(&["commit-tree", tree, "-p", &base, "-m", &msg], None);
            let executable = commit(&tree("100755", "100644"));
            let r = check(&s.bundle_head(&executable), Some(&base), &executable);
            assert_eq!(result(&r), ResultValue::Fail, "{kind} {:?}", r.diagnostics);
            assert!(
                r.diagnostics.iter().any(|d| d.rule == "CHG-02"
                    && d.is_error()
                    && d.message
                        == "canonical data file versions/b7k2m9qd/main.md has mode 100755, not 100644"),
                "{kind} {:?}",
                r.diagnostics
            );
            if kind != "migration" {
                // Outside the canonical data, the mode is not checked.
                let tool = commit(&tree("100644", "100755"));
                let r = check(&s.bundle_head(&tool), Some(&base), &tool);
                assert_eq!(
                    result(&r),
                    ResultValue::Incomplete,
                    "{kind} {:?}",
                    r.diagnostics
                );
            }
        }
    }

    /// CHG-02 `auxiliary` reads no history manifest: a commit is decided by
    /// the bytes alone when `history.yaml` is unreadable, of an unknown
    /// `schemaVersion` or names an invalid Revision ID, while every other kind
    /// stays undetermined.
    #[test]
    fn auxiliary_decides_without_a_usable_history_manifest() {
        let s = Scratch::new();
        let tree = |history: &[u8], generator: &[u8]| {
            let h = s.run(&["hash-object", "-w", "--stdin"], Some(history));
            let g = s.run(&["hash-object", "-w", "--stdin"], Some(generator));
            let spec = format!("100644 blob {h}\thistory.yaml\n100644 blob {g}\tgen.sh\n");
            s.run(&["mktree"], Some(spec.as_bytes()))
        };
        let invalid_id = format!(
            "schemaVersion: {}\nrevisions:\n- id: \"../x\"\n  text:\n    status: complete\n",
            crate::repository::HISTORY
        );
        let unusable: [&[u8]; 3] = [
            b"revisions: [\n",
            b"schemaVersion: loj/unknown\n",
            invalid_id.as_bytes(),
        ];
        for unreadable in unusable {
            let base = s.run(
                &["commit-tree", &tree(unreadable, b"a\n"), "-m", "base"],
                None,
            );
            let commit = |tree: &str, kind: &str| {
                let msg = format!("m\n\nLoJ-Change: {kind}\n");
                s.run(&["commit-tree", tree, "-p", &base, "-m", &msg], None)
            };
            let kept = commit(&tree(unreadable, b"b\n"), "auxiliary");
            let r = check(&s.bundle_head(&kept), Some(&base), &kept);
            assert_eq!(result(&r), ResultValue::Pass, "{:?}", r.diagnostics);
            assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);

            let changed = commit(&tree(b"revisions: [x\n", b"a\n"), "auxiliary");
            let r = check(&s.bundle_head(&changed), Some(&base), &changed);
            assert_eq!(result(&r), ResultValue::Fail, "{:?}", r.diagnostics);
            let found: Vec<_> = r.diagnostics.iter().map(|d| d.rule.as_str()).collect();
            assert_eq!(found, ["CHG-02"], "{:?}", r.diagnostics);
            assert!(r.diagnostics[0]
                .message
                .contains("changes canonical data file history.yaml"));

            // Other kinds read the manifest: undetermined, noted once per commit
            // although `kept` is both a child and a parent here.
            let msg = "m\n\nLoJ-Change: metadata\n";
            let head_tree = tree(unreadable, b"c\n");
            let head = s.run(&["commit-tree", &head_tree, "-p", &kept, "-m", msg], None);
            let r = check(&s.bundle_head(&head), Some(&base), &head);
            assert_eq!(result(&r), ResultValue::Incomplete, "{:?}", r.diagnostics);
            assert!(r.diagnostics.iter().all(|d| !d.is_error()));
            for c in [&kept, &head] {
                let notes = r
                    .diagnostics
                    .iter()
                    .filter(|d| d.message.starts_with(&format!("{c}:")))
                    .count();
                assert_eq!(notes, 1, "{c}: {:?}", r.diagnostics);
            }

            // Outside the change check the manifest is still required.
            let b = Bundle::open(&s.bundle_head(&kept)).unwrap();
            assert!(matches!(snapshot(&b, &kept), Err(ExportError::Tree(_))));

            // Either side alone being unusable leaves the other kinds undetermined.
            let usable = format!(
                "schemaVersion: {}\nrevisions: []\n",
                crate::repository::HISTORY
            );
            let readable = s.run(
                &[
                    "commit-tree",
                    &tree(usable.as_bytes(), b"a\n"),
                    "-m",
                    "base",
                ],
                None,
            );
            for (from, to) in [
                (unreadable, usable.as_bytes()),
                (usable.as_bytes(), unreadable),
            ] {
                let parent = if from == usable.as_bytes() {
                    &readable
                } else {
                    &base
                };
                let child = s.run(
                    &["commit-tree", &tree(to, b"d\n"), "-p", parent, "-m", msg],
                    None,
                );
                let r = check(&s.bundle_head(&child), Some(parent), &child);
                assert_eq!(result(&r), ResultValue::Incomplete, "{:?}", r.diagnostics);
                let side = if to == unreadable { &child } else { parent };
                let found: Vec<_> = r
                    .diagnostics
                    .iter()
                    .map(|d| (d.class, &d.message))
                    .collect();
                assert!(
                    matches!(found.as_slice(), [(Class::Undetermined, m)] if m.starts_with(&format!("{side}:"))),
                    "{found:?}"
                );
            }
        }
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
        assert!(snap.manifest_error.is_none());
    }

    /// Without `base`, every commit reachable from `head` is checked from the
    /// commits without a parent on. In a history with two roots, the merge
    /// commit joining them and each root that does not declare `revisionAdd`
    /// fail CHG-03 without being compared, so no CHG-02 finding (not even the
    /// mode of the executable file) is reported.
    #[test]
    fn without_base_the_whole_history_from_each_root_is_checked() {
        let s = Scratch::new();
        let blob = s.run(&["hash-object", "-w", "--stdin"], Some(b""));
        let tree = |mode: &str| {
            let spec = format!("{mode} blob {blob}\thistory.yaml\n");
            s.run(&["mktree"], Some(spec.as_bytes()))
        };
        let (plain, executable) = (tree("100644"), tree("100755"));
        let commit = |tree: &str, parents: &[&str], msg: &str| {
            let mut args = vec!["commit-tree", tree];
            for p in parents {
                args.extend(["-p", p]);
            }
            args.extend(["-m", msg]);
            s.run(&args, None)
        };
        let first = commit(&plain, &[], "first\n\nLoJ-Change: format\n");
        let other = commit(&executable, &[], "other\n\nLoJ-Change: metadata\n");
        let merge = commit(&plain, &[&first, &other], "m\n\nLoJ-Change: format\n");
        let r = check(&s.bundle_head(&merge), None, &merge);
        assert_eq!(result(&r), ResultValue::Fail, "{:?}", r.diagnostics);
        let errors: Vec<_> = r
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .map(|d| (d.rule.as_str(), d.message.as_str()))
            .collect();
        assert!(
            errors.iter().all(|(rule, _)| *rule == "CHG-03"),
            "{errors:?}"
        );
        assert!(
            errors.contains(&("CHG-03", "merge commits are not permitted")),
            "{errors:?}"
        );
        assert_eq!(
            errors
                .iter()
                .filter(|e| e.1 == "first law commit must declare revisionAdd")
                .count(),
            2,
            "{errors:?}"
        );
        let at = |c: &str| at_of(&r, c);
        assert_eq!(
            (at(&first), at(&other), at(&merge)),
            (1, 1, 1),
            "{errors:?}"
        );
        // With `base`, the commits up to it are not checked.
        let r = check(&s.bundle_head(&merge), Some(&first), &merge);
        assert_eq!(at_of(&r, &first), 0, "{:?}", r.diagnostics);
        assert_eq!(at_of(&r, &other), 1, "{:?}", r.diagnostics);
    }

    /// A commit with two children is compared with each of them, although its
    /// snapshot is dropped from the cache after its last comparison.
    #[test]
    fn a_parent_of_two_children_is_compared_with_each() {
        let s = Scratch::new();
        let blob = s.run(&["hash-object", "-w", "--stdin"], Some(b""));
        let tree = |mode: &str| {
            let spec = format!("{mode} blob {blob}\thistory.yaml\n");
            s.run(&["mktree"], Some(spec.as_bytes()))
        };
        let (plain, executable) = (tree("100644"), tree("100755"));
        let commit = |tree: &str, parents: &[&str], msg: &str| {
            let mut args = vec!["commit-tree", tree];
            for p in parents {
                args.extend(["-p", p]);
            }
            args.extend(["-m", msg]);
            s.run(&args, None)
        };
        let root = commit(&plain, &[], "root\n\nLoJ-Change: revisionAdd\n");
        let left = commit(&executable, &[&root], "left\n\nLoJ-Change: metadata\n");
        let right = commit(&executable, &[&root], "right\n\nLoJ-Change: metadata\n");
        let merge = commit(&plain, &[&left, &right], "m\n\nLoJ-Change: format\n");
        let r = check(&s.bundle_head(&merge), None, &merge);
        let mode = |c: &str| {
            r.diagnostics.iter().any(|d| {
                d.rule == "CHG-02"
                    && d.message.contains("has mode 100755")
                    && d.location.as_ref().and_then(|l| l.get("file"))
                        == Some(&json!(format!("commit {c}")))
            })
        };
        assert!(mode(&left) && mode(&right), "{:?}", r.diagnostics);
        assert_eq!(at_of(&r, &merge), 1, "{:?}", r.diagnostics);
    }

    /// A refused parent with two children is noted once, although both
    /// children compare with it and its cache entry is dropped after the last.
    #[test]
    fn a_refused_parent_of_two_children_is_noted_once() {
        let s = Scratch::new();
        let blob = s.run(&["hash-object", "-w", "--stdin"], Some(b""));
        let tree = |name: &str| {
            let spec = format!("100644 blob {blob}\t{name}\n");
            s.run(&["mktree"], Some(spec.as_bytes()))
        };
        let (good, bad) = (tree("history.yaml"), tree("a:b"));
        let commit = |tree: &str, parents: &[&str], msg: &str| {
            let mut args = vec!["commit-tree", tree];
            for p in parents {
                args.extend(["-p", p]);
            }
            args.extend(["-m", msg]);
            s.run(&args, None)
        };
        let root = commit(&bad, &[], "root\n\nLoJ-Change: revisionAdd\n");
        let left = commit(&good, &[&root], "left\n\nLoJ-Change: metadata\n");
        let right = commit(&good, &[&root], "right\n\nLoJ-Change: metadata\n");
        let merge = commit(&good, &[&left, &right], "m\n\nLoJ-Change: format\n");
        let r = check(&s.bundle_head(&merge), None, &merge);
        assert_eq!(result(&r), ResultValue::Fail, "{:?}", r.diagnostics);
        let notes = r
            .diagnostics
            .iter()
            .filter(|d| d.message.starts_with(&format!("{root}: tree refused")))
            .count();
        assert_eq!(notes, 1, "{:?}", r.diagnostics);
    }

    /// The check with `limits` in place of [`LIMITS`].
    fn check_with(path: &Path, base: Option<&str>, head: &str, limits: &Limits) -> Report {
        let b = Bundle::open(path).unwrap();
        check_bundle(
            &b,
            Findings::default(),
            Report::default(),
            base,
            head,
            RepositoryKind::Law,
            limits,
        )
    }

    fn undetermined_by(r: &Report, message: &str) -> bool {
        r.diagnostics
            .iter()
            .any(|d| d.rule == "CHG-02" && d.class == Class::Undetermined && d.message == message)
    }

    fn limits(commits: usize, duration: Duration) -> Limits {
        Limits {
            commits,
            duration,
            elapsed: LIMITS.elapsed,
        }
    }

    /// Up to the commit limit every commit is checked; one commit more and none
    /// is, so the check is incomplete without the finding of any commit, with
    /// or without `base`.
    #[test]
    fn the_commit_limit_makes_the_check_incomplete() {
        let s = Scratch::new();
        let (path, c) = s.chain(&[
            b"first\n\nLoJ-Change: revisionAdd\n",
            b"second\n\nLoJ-Change: metadata\n",
            b"third\n",
        ]);
        // Three commits without base, two after `base`: at the limit, the
        // third commit's missing `LoJ-Change` is found.
        for (base, n) in [(None, 3), (Some(c[0].as_str()), 2)] {
            let r = check_with(&path, base, &c[2], &limits(n, LIMITS.duration));
            assert_eq!(result(&r), ResultValue::Fail, "{:?}", r.diagnostics);
            assert_eq!(at_of(&r, &c[2]), 1, "{:?}", r.diagnostics);
            let r = check_with(&path, base, &c[2], &limits(n - 1, LIMITS.duration));
            assert_eq!(result(&r), ResultValue::Incomplete, "{:?}", r.diagnostics);
            assert!(
                r.diagnostics.iter().all(|d| !d.is_error()),
                "{:?}",
                r.diagnostics
            );
            let why = format!("more than {} commits to check", n - 1);
            assert!(undetermined_by(&r, &why), "{:?}", r.diagnostics);
            assert_eq!(r.target["commit"], c[2]);
        }
    }

    /// Past the time limit, the check stops before the next commit with an
    /// undetermined note. With no finding yet it is incomplete.
    #[test]
    fn the_time_limit_makes_the_check_incomplete() {
        let s = Scratch::new();
        let (path, c) = s.chain(&[b"first\n\nLoJ-Change: revisionAdd\n", b"second\n"]);
        let r = check_with(&path, None, &c[1], &limits(LIMITS.commits, Duration::ZERO));
        assert_eq!(result(&r), ResultValue::Incomplete, "{:?}", r.diagnostics);
        assert!(
            r.diagnostics.iter().all(|d| !d.is_error()),
            "{:?}",
            r.diagnostics
        );
        let why = "the change check ran longer than 0 s";
        assert!(undetermined_by(&r, why), "{:?}", r.diagnostics);
        // Within the limit, the same input is decided.
        let r = check_with(&path, None, &c[1], &LIMITS);
        assert_eq!(result(&r), ResultValue::Fail, "{:?}", r.diagnostics);
    }

    /// A finding made before the time limit passed is kept: a violation found
    /// is a violation whatever the unchecked commits hold, so the check fails
    /// (fail > incomplete) and the note says why the rest was not checked.
    #[test]
    fn a_finding_before_the_time_limit_still_fails_the_check() {
        thread_local! {
            static CALLS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
        }
        // No time has passed before the first commit; the limit has passed
        // before the second.
        fn clock(_: Instant) -> Duration {
            CALLS.with(|n| {
                n.set(n.get() + 1);
                if n.get() == 1 {
                    Duration::ZERO
                } else {
                    Duration::MAX
                }
            })
        }
        let s = Scratch::new();
        let (path, c) = s.chain(&[b"first\n", b"second\n"]);
        let limits = Limits {
            elapsed: clock,
            ..limits(LIMITS.commits, LIMITS.duration)
        };
        let r = check_with(&path, None, &c[1], &limits);
        assert_eq!(CALLS.with(std::cell::Cell::get), 2);
        assert_eq!(result(&r), ResultValue::Fail, "{:?}", r.diagnostics);
        assert_eq!(at_of(&r, &c[0]), 1, "{:?}", r.diagnostics);
        assert_eq!(at_of(&r, &c[1]), 0, "{:?}", r.diagnostics);
        let why = "the change check ran longer than 3600 s";
        assert!(undetermined_by(&r, why), "{:?}", r.diagnostics);
    }

    /// The errors reported at `commit` (the change check's `location.file`).
    fn at_of(r: &Report, commit: &str) -> usize {
        let file = format!("commit {commit}");
        r.diagnostics
            .iter()
            .filter(|d| {
                d.is_error()
                    && d.location
                        .as_ref()
                        .and_then(|l| l.get("file"))
                        .and_then(Value::as_str)
                        == Some(file.as_str())
            })
            .count()
    }

    #[test]
    fn registry_metadata_ignores_an_executable_invalid_law_manifest() {
        let s = Scratch::new();
        let blob = s.run(&["hash-object", "-w", "--stdin"], Some(b"bad: ["));
        let tree = s.run(
            &["mktree"],
            Some(format!("100755 blob {blob}\thistory.yaml\n").as_bytes()),
        );
        let head = s.run(
            &[
                "commit-tree",
                &tree,
                "-m",
                "registry root\n\nLoJ-Change: metadata",
            ],
            None,
        );
        let r = check_repository(&s.bundle_head(&head), None, &head, RepositoryKind::Registry);
        assert_eq!(result(&r), ResultValue::Pass, "{:?}", r.diagnostics);
    }

    #[test]
    fn only_the_law_root_may_create_whole_rebuild_metadata_as_revision_add() {
        let s = Scratch::new();
        let blob = s.run(
            &["hash-object", "-w", "--stdin"],
            Some(b"schemaVersion: loj-rebuild/v1alpha1"),
        );
        let tree = s.run(
            &["mktree"],
            Some(format!("100644 blob {blob}\trebuild.yaml\n").as_bytes()),
        );
        let message = "root\n\nLoJ-Change: revisionAdd";
        let head = s.run(&["commit-tree", &tree, "-m", message], None);
        let r = check(&s.bundle_head(&head), None, &head);
        assert_eq!(result(&r), ResultValue::Pass, "{:?}", r.diagnostics);
        let empty = s.run(&["mktree"], Some(b""));
        let root = s.run(&["commit-tree", &empty, "-m", message], None);
        let child = s.run(&["commit-tree", &tree, "-p", &root, "-m", message], None);
        let r = check(&s.bundle_head(&child), None, &child);
        assert!(r
            .diagnostics
            .iter()
            .any(|d| d.rule == "CHG-02" && d.message.contains("unrelated rebuild data")));
    }

    /// Without `base`, a lone first commit is compared with no record before
    /// it: `revisionAdd` of an executable canonical data file fails CHG-02.
    #[test]
    fn without_base_a_first_commit_is_checked_against_no_record() {
        let s = Scratch::new();
        let blob = s.run(&["hash-object", "-w", "--stdin"], Some(b""));
        let spec = format!("100755 blob {blob}\thistory.yaml\n");
        let tree = s.run(&["mktree"], Some(spec.as_bytes()));
        let msg = "first\n\nLoJ-Change: revisionAdd\n";
        let first = s.run(&["commit-tree", &tree, "-m", msg], None);
        let r = check(&s.bundle_head(&first), None, &first);
        assert_eq!(result(&r), ResultValue::Fail, "{:?}", r.diagnostics);
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.rule == "CHG-02" && d.message.contains("has mode 100755")),
            "{:?}",
            r.diagnostics
        );
        assert_eq!(r.target["commit"], first);
        // A `base` missing from the bundle stays undetermined.
        let r = check(&s.bundle_head(&first), Some(&"0".repeat(40)), &first);
        assert_eq!(result(&r), ResultValue::Incomplete, "{:?}", r.diagnostics);
    }
}
