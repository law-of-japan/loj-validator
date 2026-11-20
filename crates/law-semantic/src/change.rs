//! Change check between two commits of a law repository (common/v1alpha1
//! CHG-02, CHG-03, CHG-04), reported apart from the conformance levels.
use crate::common::findings::Findings;
use crate::common::{load_yaml, Check, Class, Loaded, Report, COMMON};
use crate::git::{Bundle, ExportError, GitError};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const CH: Check = Check::CHANGE;
mod policy;
const KINDS: &[&str] = &[
    "format",
    "correction",
    "officialCorrection",
    "restoration",
    "metadata",
    "revisionAdd",
    "revisionFix",
    "migration",
    "auxiliary",
];

/// The state of a record version at one commit.
#[derive(Default)]
pub(crate) struct Snapshot {
    pub history: Option<Value>,
    pub history_bytes: Option<Vec<u8>>,
    /// revision_id -> AST, or why no AST could be built (CHG-04).
    pub asts: BTreeMap<String, Result<Value, String>>,
    /// revision_id -> package path.
    pub packages: BTreeMap<String, String>,
    pub files: BTreeMap<String, [u8; 32]>,
    /// Paths whose Git file mode is executable (`100755`); every other file is
    /// `100644`, as the export refuses any other mode.
    pub executable: BTreeSet<String>,
    pub documents: BTreeMap<String, Value>,
    /// Why the history manifest could not be used (unreadable, an unsupported
    /// `schemaVersion`, an invalid Revision ID), if so. `history` and
    /// `packages` are then empty, while the file lists are complete, which
    /// is all `auxiliary` compares (CHG-02).
    pub manifest_error: Option<String>,
    /// Export retained until Bundle is dropped. Deferred AST generation must
    /// finish while that bundle is alive (the change-check cache is local to it).
    directory: std::path::PathBuf,
}

/// A snapshot whose history manifest is usable; an unusable one is a refused
/// tree (`ExportError::Tree`).
pub(crate) fn snapshot(b: &Bundle, commit: &str) -> Result<Snapshot, ExportError> {
    let mut snapshot = read_snapshot(b, commit)?;
    if let Some(e) = snapshot.manifest_error.take() {
        return Err(ExportError::Tree(e));
    }
    snapshot.build_asts(|_| true);
    Ok(snapshot)
}

fn read_snapshot(b: &Bundle, commit: &str) -> Result<Snapshot, ExportError> {
    let (dir, executable) = b.export_tree(commit)?;
    let mut files = BTreeMap::new();
    let mut documents = BTreeMap::new();
    let mut pending = vec![dir.clone()];
    while let Some(directory) = pending.pop() {
        if crate::cancellation::checkpoint() {
            return Err(ExportError::Cancelled);
        }
        for entry in std::fs::read_dir(directory).map_err(|e| ExportError::Io(e.to_string()))? {
            if crate::cancellation::checkpoint() {
                return Err(ExportError::Cancelled);
            }
            let entry = entry.map_err(|e| ExportError::Io(e.to_string()))?;
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let bytes = std::fs::read(&path).map_err(|e| ExportError::Io(e.to_string()))?;
                let relative = path
                    .strip_prefix(&dir)
                    .expect("exported tree entry is inside its root")
                    .to_string_lossy()
                    .replace('\\', "/");
                if relative.ends_with(".yaml") {
                    if let Ok(text) = std::str::from_utf8(&bytes) {
                        if let Ok(value) = crate::common::yaml::parse(text) {
                            documents.insert(relative.clone(), value);
                        }
                    }
                }
                files.insert(relative, Sha256::digest(&bytes).into());
            }
        }
    }
    // The export holds every blob byte for byte, so the raw manifest is read from
    // it rather than from git again. No file of that name (absent, or a directory
    // in the tree) is no manifest, a finding about the data; failing to look at or
    // read the file leaves the check undetermined.
    let manifest = dir.join("history.yaml");
    let unreadable =
        |e: std::io::Error| ExportError::Io(format!("cannot read history.yaml at {commit}: {e}"));
    let history_bytes = match std::fs::metadata(&manifest) {
        Ok(m) if m.is_file() => Some(std::fs::read(&manifest).map_err(unreadable)?),
        Ok(_) => None,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(unreadable(e)),
    };
    let mut snapshot = Snapshot {
        history: None,
        history_bytes,
        asts: BTreeMap::new(),
        packages: BTreeMap::new(),
        files,
        executable,
        documents,
        manifest_error: None,
        directory: dir,
    };
    match read_manifest(&snapshot.directory, snapshot.history_bytes.is_some()) {
        Ok((history, packages)) => (snapshot.history, snapshot.packages) = (history, packages),
        Err(e) => snapshot.manifest_error = Some(e),
    }
    Ok(snapshot)
}

type Packages = BTreeMap<String, String>;

/// The history manifest of an exported tree and the package of each revision
/// with text, or why the manifest cannot be used.
fn read_manifest(dir: &Path, present: bool) -> Result<(Option<Value>, Packages), String> {
    let history = match load_yaml(&dir.join("history.yaml")) {
        Loaded::Ok(v, _) if v["schemaVersion"] == crate::repository::HISTORY => Some(v),
        _ if present => {
            return Err("history manifest is unreadable or has an unsupported schemaVersion".into())
        }
        _ => None,
    };
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
        if !crate::repository::history::valid_revision_id(id) {
            return Err("invalid Revision ID cannot select a package".into());
        }
        let path = crate::repository::history::package_path(r);
        packages.insert(id.to_owned(), path);
    }
    Ok((history, packages))
}

impl Snapshot {
    /// CHG-02: declared corrections/restorations do not build an AST. A cached
    /// commit may need that AST later, as the parent of a different change.
    fn build_asts(&mut self, needed: impl Fn(&str) -> bool) {
        for revision in self
            .history
            .as_ref()
            .and_then(|h| h["revisions"].as_array())
            .into_iter()
            .flatten()
        {
            let Some(id) = revision["id"].as_str() else {
                continue;
            };
            // Leaving early is safe: the report is cancelled by
            // `checks::run_with_space` before it is returned.
            if crate::cancellation::checkpoint() {
                break;
            }
            let Some(path) = self.packages.get(id) else {
                continue;
            };
            if !needed(id) || self.asts.contains_key(id) {
                continue;
            }
            let ast = if revision["text"]["status"] == "partial" {
                Err("partial text".into())
            } else {
                let pkg =
                    crate::v1alpha2::check(&self.directory.join(path), &[Check::L0, Check::L1]);
                pkg.ast.ok_or_else(|| "the package does not pass L1".into())
            };
            self.asts.insert(id.to_owned(), ast);
        }
    }
}

/// CHG-03: only a wholly valid final paragraph, excluding the first one.
fn trailer_fields(message: &str) -> Vec<(&str, &str)> {
    let mut paragraphs: Vec<Vec<&str>> = Vec::new();
    let mut current = Vec::new();
    for line in message
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
    {
        if line.trim_matches([' ', '\t']).is_empty() {
            if !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }
    if paragraphs.len() < 2 {
        return Vec::new();
    }
    let mut fields = Vec::new();
    for line in paragraphs.last().into_iter().flatten() {
        let Some((name, value)) = line.split_once(':') else {
            return Vec::new();
        };
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Vec::new();
        }
        fields.push((name, value.trim_matches([' ', '\t'])));
    }
    fields
}

fn trailers(message: &str) -> (Vec<String>, Vec<String>) {
    let fields = trailer_fields(message);
    let kinds = fields
        .iter()
        .filter(|(name, _)| *name == "LoJ-Change")
        .map(|(_, value)| (*value).into())
        .collect();
    let revisions: Vec<String> = fields
        .iter()
        .filter(|(name, _)| *name == "LoJ-Revisions")
        .map(|(_, value)| (*value).into())
        .collect();
    (kinds, revisions)
}

fn reviewed(message: &str) -> bool {
    let fields = trailer_fields(message);
    let values: Vec<_> = fields
        .iter()
        .filter(|(name, _)| *name == "LoJ-Reviewed")
        .collect();
    matches!(values.as_slice(), [(_, value)] if !value.is_empty())
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
    // Commits whose unusable history manifest has been noted, once each.
    let mut noted: BTreeSet<String> = BTreeSet::new();
    for (commit, parents) in commits {
        if crate::cancellation::checkpoint() {
            return undetermined(f, report.target, GitError::Cancelled.to_string());
        }
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
            continue;
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
                continue;
            }
            Err(e) => return undetermined(f, report.target, format!("{commit}: {e}")),
        };
        let Some((kind, declared)) = declared_change(&commit, &at, &message, &mut f) else {
            continue;
        };
        let parent = parents.first();
        if parent.is_none() && kind != "revisionAdd" {
            f.error(
                CH,
                "CHG-03",
                COMMON,
                "first law commit must declare revisionAdd",
                &at,
                "",
            );
            continue;
        }
        let change = Change {
            commit: &commit,
            at: &at,
            kind: &kind,
            declared: &declared,
            reviewed: reviewed(&message),
        };
        for c in parent.into_iter().chain(std::iter::once(&commit)) {
            if let Err(why) = load_snapshot(b, c, &change, &mut cache, &mut f) {
                return undetermined(f, report.target, why);
            }
        }
        let empty = Snapshot::default();
        let before = match parent {
            Some(parent) => match cache.get(parent) {
                Some(Some(snapshot)) => snapshot,
                _ => continue,
            },
            None => &empty,
        };
        let Some(Some(after)) = cache.get(&commit) else {
            continue;
        };
        // CHG-02 `auxiliary` compares the files only, so it is decided even
        // when a history manifest cannot be used; every other kind reads it.
        if kind != "auxiliary" {
            let unusable: Vec<_> = parent
                .into_iter()
                .zip([before])
                .chain([(&commit, after)])
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
                continue;
            }
        }
        change.compare(before, after, &mut f);
    }
    let mut r = f.into_report(&[CH]);
    r.target = report.target;
    r
}

/// CHG-03: the commit declares exactly one known `LoJ-Change`. Returns the
/// kind and the `LoJ-Revisions`, or `None` when the declarations are invalid
/// and the trees must not be compared. Revision trailer lines are not combined.
fn declared_change(
    commit: &str,
    at: &str,
    message: &str,
    f: &mut Findings,
) -> Option<(String, Vec<String>)> {
    let (kinds, revisions) = trailers(message);
    if revisions.len() > 1 {
        f.error(
            CH,
            "CHG-03",
            COMMON,
            format!("{commit} declares more than one LoJ-Revisions"),
            at,
            "",
        );
        return None;
    }
    let problem = match kinds.as_slice() {
        [k] if KINDS.contains(&k.as_str()) => {
            let declared = revisions
                .first()
                .map(|value| value.split(", ").map(str::to_owned).collect())
                .unwrap_or_default();
            return Some((k.clone(), declared));
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
    change: &Change<'_>,
    cache: &mut BTreeMap<String, Option<Snapshot>>,
    f: &mut Findings,
) -> Result<(), String> {
    if let Some(cached) = cache.get_mut(commit) {
        if let Some(snapshot) = cached {
            snapshot.build_asts(|id| change.must_keep(id));
        }
        return Ok(());
    }
    match read_snapshot(b, commit) {
        Ok(mut s) => {
            s.build_asts(|id| change.must_keep(id));
            cache.insert(commit.to_owned(), Some(s));
        }
        // Cancellation stops the check like an environment failure; the report
        // is then made incomplete by `cancellation::finish`.
        Err(e @ (ExportError::Environment(_) | ExportError::Io(_) | ExportError::Cancelled)) => {
            return Err(format!("{commit}: {e}"))
        }
        Err(ExportError::Tree(e)) => {
            f.incomplete.insert(CH);
            f.note(
                CH,
                Class::Undetermined,
                "CHG-02",
                COMMON,
                format!("{commit}: tree refused: {e}"),
                change.at,
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
    reviewed: bool,
}

impl Change<'_> {
    fn error(&self, f: &mut Findings, rule: &str, message: String) {
        f.error(CH, rule, COMMON, message, self.at, "");
    }

    /// CHG-02 / CHG-03 / CHG-04: compare the states before and after the commit.
    fn compare(&self, before: &Snapshot, after: &Snapshot, f: &mut Findings) {
        let (commit, kind) = (self.commit, self.kind);
        if kind == "migration" {
            self.error(
                f,
                "CHG-02",
                "this specification provides no normative AST migration mapping".into(),
            );
            return;
        }
        if kind == "auxiliary" {
            // CHG-02: only the canonical data files are compared, byte for byte
            // and by mode. No AST is built or compared, so a version that cannot
            // build one is not undetermined here (CHG-04).
            policy::auxiliary(self, before, after, f);
            return;
        }
        if !policy::processable(self, before, f) || !policy::processable(self, after, f) {
            return;
        }
        self.kept_asts(before, after, f);
        policy::check(self, before, after, f);
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
            "revisionAdd"
                if policy::revision_ids(after)
                    .iter()
                    .all(|k| policy::revision_ids(before).contains(k)) =>
            {
                self.error(
                    f,
                    "CHG-02",
                    format!("{commit}: revisionAdd adds no version"),
                )
            }
            _ => {}
        }
    }

    /// Whether the AST of revision `id` must stay identical under this change.
    fn must_keep(&self, id: &str) -> bool {
        match self.kind {
            "format" | "metadata" | "revisionAdd" => true,
            "auxiliary" => false,
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
                    let prefix = format!("versions/{id}/");
                    let changed = before.files.keys().chain(after.files.keys()).any(|path| {
                        path.starts_with(&prefix)
                            && !path.ends_with("/glyphs.yaml")
                            && before.files.get(path) != after.files.get(path)
                    });
                    if kind == "format" && changed && !self.reviewed {
                        self.error(f, "CHG-04", format!("{commit}: format changes uncomparable revision {id} without a review record"));
                    }
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
        let report = check(&path, &commits[0], &commits[1]);
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
                    commit,
                    at: "test commit",
                    kind,
                    declared: &declared,
                    reviewed: false,
                };
                let mut cache = BTreeMap::new();
                let mut f = Findings::default();
                load_snapshot(&b, commit, &change, &mut cache, &mut f).unwrap();
                let cached = cache[commit].as_ref().unwrap();
                assert!(cached.asts.is_empty());
                assert!(cached.packages.contains_key(&declared[0]));
                let directory = cached.directory.clone();

                // The next commit requires all ASTs, even if it declares IDs.
                change.kind = "metadata";
                load_snapshot(&b, commit, &change, &mut cache, &mut f).unwrap();
                let cached = cache[commit].as_ref().unwrap();
                assert_eq!(cached.directory, directory);
                assert_eq!(cached.asts.len(), 1);
                assert_eq!(cached.asts, snapshot(&b, commit).unwrap().asts);
                assert!(f.items.is_empty());

                // Undeclared versions must still be checked during correction.
                change.kind = kind;
                change.declared = &[];
                let mut other_cache = BTreeMap::new();
                load_snapshot(&b, commit, &change, &mut other_cache, &mut f).unwrap();
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

    /// Cancellation observed while walking the exported files is reported as a
    /// cancellation, not as a git or file-system failure.
    #[test]
    fn snapshot_cancellation_is_its_own_kind() {
        let s = Scratch::new();
        let b = Bundle::open(&s.bundle(&[("100644", "blob", "law.yaml")])).unwrap();
        let head = b.branch("main").unwrap().unwrap();
        let (whole, checkpoints) =
            crate::cancellation::test_hook::run(None, || read_snapshot(&b, &head).is_ok());
        assert!(whole);
        assert!(checkpoints > 0, "the walk has checkpoints");
        for at in 1..=checkpoints {
            let (result, _) =
                crate::cancellation::test_hook::run(Some(at), || read_snapshot(&b, &head));
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
            let r = check(&s.bundle_head(&head), &base, &head);
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
            let r = check(&s.bundle_head(&kept), &base, &kept);
            assert_eq!(result(&r), ResultValue::Pass, "{:?}", r.diagnostics);
            assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);

            let changed = commit(&tree(b"revisions: [x\n", b"a\n"), "auxiliary");
            let r = check(&s.bundle_head(&changed), &base, &changed);
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
            let r = check(&s.bundle_head(&head), &base, &head);
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
    }
}
