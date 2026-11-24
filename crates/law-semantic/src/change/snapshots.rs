//! Acquisition and caching of commit snapshots for change and reference checks.
use super::{Change, RepositoryKind, CH};
use crate::common::findings::Findings;
use crate::common::{load_yaml, Check, Class, Loaded, COMMON};
use crate::git::{Bundle, ExportError};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

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
    pub(super) directory: std::path::PathBuf,
}

/// A snapshot whose history manifest is usable; an unusable one is a refused
/// tree (`ExportError::Tree`).
pub(crate) fn snapshot(b: &Bundle, commit: &str) -> Result<Snapshot, ExportError> {
    let mut snapshot = read_snapshot(b, commit, None)?;
    if let Some(e) = snapshot.manifest_error.take() {
        return Err(ExportError::Tree(e));
    }
    snapshot.build_asts(|_| true);
    Ok(snapshot)
}

pub(super) fn read_snapshot(
    b: &Bundle,
    commit: &str,
    repository: Option<RepositoryKind>,
) -> Result<Snapshot, ExportError> {
    let (dir, mut executable) = b.export_tree(commit)?;
    if let Some(layout) = repository {
        executable.retain(|path| layout.canonical(path));
    }
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
                if repository.is_some_and(|r| !r.canonical(&relative)) {
                    continue;
                }
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
    let history_bytes = if repository == Some(RepositoryKind::Registry) {
        None
    } else {
        match std::fs::metadata(&manifest) {
            Ok(m) if m.is_file() => Some(std::fs::read(&manifest).map_err(unreadable)?),
            Ok(_) => None,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(unreadable(e)),
        }
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
    if repository == Some(RepositoryKind::Registry) {
        return Ok(snapshot);
    }
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

/// Export `commit` unless the cache has it. A tree refused for its content is
/// noted and cached as `None`. `Err` is a failure of the environment, which
/// ends the whole check.
pub(super) fn load_snapshot(
    b: &Bundle,
    commit: &str,
    repository: RepositoryKind,
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
    match read_snapshot(b, commit, Some(repository)) {
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
