//! Minimal read-only access to a `git bundle` through the `git` command.
//! Blobs are read raw (no line-ending conversion), so byte positions hold.
//! A bundle is untrusted input: tree entry names are checked before anything is
//! written, and only regular files are exported.
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// A bare repository fetched from a bundle into a temporary directory.
pub struct Bundle {
    dir: PathBuf,
    exports: std::cell::RefCell<Vec<PathBuf>>,
}

/// Create a fresh, empty temporary directory. `create_dir` fails if the name is
/// already taken, so a directory planted in the shared temp location is never reused.
fn temp(prefix: &str) -> Result<PathBuf, String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let base = std::env::temp_dir();
    for _ in 0..16 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = base.join(format!(
            "{prefix}-{}-{nanos}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("cannot create a temporary directory: {e}")),
        }
    }
    Err("cannot create a fresh temporary directory".into())
}

/// Reject a tree entry name that could escape the export directory or that is not
/// portable: empty, absolute, `.`/`..`/empty components, a `.git` component (any case),
/// a `:` or `\`, and the Windows/macOS aliases of `.git` (a component ending in `.` or
/// a space, or the short name `GIT~1`).
fn safe_name(name: &str) -> Result<(), String> {
    let bad = || Err(format!("unsafe path in the bundle tree: {name:?}"));
    if name.is_empty() || name.contains(['\\', ':']) || name.starts_with('/') {
        return bad();
    }
    if name.split('/').any(|c| {
        c.is_empty()
            || c == "."
            || c == ".."
            || c.ends_with(['.', ' '])
            || c.eq_ignore_ascii_case(".git")
            || c.eq_ignore_ascii_case("git~1")
    }) {
        return bad();
    }
    if !Path::new(name)
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
    {
        return bad();
    }
    Ok(())
}

#[cfg(windows)]
const NULL_DEVICE: &str = "NUL";
#[cfg(not(windows))]
const NULL_DEVICE: &str = "/dev/null";

/// A revision argument from the caller (CLI, reference string). Refuse values git
/// could read as an option, or that carry whitespace or control characters.
fn revision(rev: &str) -> Result<(), String> {
    if rev.is_empty()
        || rev.starts_with('-')
        || rev.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(format!("not a revision: {rev:?}"));
    }
    Ok(())
}

/// Strip a verbatim `\\?\` prefix, which git would read as a host name.
#[cfg(windows)]
fn git_path(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy().into_owned();
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path,
    }
}
#[cfg(not(windows))]
fn git_path(path: PathBuf) -> PathBuf {
    path
}

impl Bundle {
    /// Fetch every branch of the bundle. `Err` means the input is not available.
    pub fn open(bundle: &Path) -> Result<Bundle, String> {
        let bundle = git_path(
            bundle
                .canonicalize()
                .map_err(|e| format!("bundle unreadable: {e}"))?,
        );
        let b = Bundle {
            dir: temp("loj-bundle")?,
            exports: Default::default(),
        };
        b.git(&["init", "--quiet", "--bare", "."])?;
        b.git(&[
            "-c",
            "transfer.fsckObjects=true",
            "-c",
            "fetch.fsckObjects=true",
            "fetch",
            "--quiet",
            &bundle.to_string_lossy(),
            "+refs/heads/*:refs/heads/*",
        ])?;
        Ok(b)
    }
    /// Run git in the bundle's repository, isolated from the caller's git
    /// environment (`GIT_DIR`, `GIT_CONFIG_*`, …) and from the user and system
    /// configuration, so the result depends only on the bundle.
    fn git(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        let mut cmd = Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                cmd.env_remove(key);
            }
        }
        let out = cmd
            .arg("-C")
            .arg(&self.dir)
            .args(["-c", "core.autocrlf=false", "-c", "core.safecrlf=false"])
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", NULL_DEVICE)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|e| format!("git is not available: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
        }
        Ok(out.stdout)
    }
    fn text(&self, args: &[&str]) -> Option<String> {
        self.git(args)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .map(|s| s.trim().to_owned())
    }
    /// The full hash of an existing commit, if the object is present.
    pub fn commit(&self, rev: &str) -> Option<String> {
        revision(rev).ok()?;
        self.text(&[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{rev}^{{commit}}"),
        ])
    }
    pub fn branch(&self, name: &str) -> Option<String> {
        self.commit(&format!("refs/heads/{name}"))
    }
    pub fn is_ancestor(&self, a: &str, b: &str) -> bool {
        revision(a).is_ok()
            && revision(b).is_ok()
            && self.git(&["merge-base", "--is-ancestor", a, b]).is_ok()
    }
    /// Commits of `base..head`, oldest first, each with its first parent.
    pub fn range(&self, base: &str, head: &str) -> Option<Vec<(String, Option<String>)>> {
        revision(base).ok()?;
        revision(head).ok()?;
        let list = self.text(&[
            "rev-list",
            "--reverse",
            "--parents",
            &format!("{base}..{head}"),
        ])?;
        Some(
            list.lines()
                .filter(|l| !l.is_empty())
                .map(|l| {
                    let mut w = l.split(' ');
                    let c = w.next().unwrap_or("").to_owned();
                    (c, w.next().map(str::to_owned))
                })
                .collect(),
        )
    }
    /// The raw commit message (UTF-8, CHG-03). The header ends at the first empty
    /// line; continuation lines of multi-line headers (e.g. `gpgsig`) start with a space.
    pub fn message(&self, commit: &str) -> Option<String> {
        revision(commit).ok()?;
        let raw = self.git(&["cat-file", "commit", commit]).ok()?;
        let raw = String::from_utf8(raw).ok()?;
        raw.split_once("\n\n").map(|(_, m)| m.to_owned())
    }
    /// A blob at `commit:path`, raw.
    pub fn blob(&self, commit: &str, path: &str) -> Option<Vec<u8>> {
        revision(commit).ok()?;
        self.git(&["cat-file", "blob", &format!("{commit}:{path}")])
            .ok()
    }
    /// Write the tree of `commit` to a fresh directory, byte for byte. Every entry
    /// name is checked before anything is written; symbolic links, submodules and
    /// non-UTF-8 names are refused with an `Err`, never followed or written.
    pub fn export(&self, commit: &str) -> Result<PathBuf, String> {
        revision(commit)?;
        let listing = self.git(&["ls-tree", "-r", "-z", "--full-tree", commit])?;
        let mut entries = Vec::new();
        for entry in listing.split(|b| *b == 0).filter(|n| !n.is_empty()) {
            let entry = String::from_utf8(entry.to_vec()).map_err(|_| {
                format!(
                    "non-UTF-8 path in the bundle tree: {:?}",
                    String::from_utf8_lossy(entry)
                )
            })?;
            let (meta, name) = entry
                .split_once('\t')
                .ok_or_else(|| format!("unexpected ls-tree entry: {entry:?}"))?;
            let mode = meta.split(' ').next().unwrap_or("");
            safe_name(name)?;
            if !matches!(mode, "100644" | "100755") {
                return Err(format!(
                    "unsupported tree entry (mode {mode}) in the bundle: {name:?}"
                ));
            }
            entries.push(name.to_owned());
        }
        // Names (and directory prefixes) that differ only by case would overwrite
        // each other on a case-insensitive file system.
        let mut folded = std::collections::BTreeMap::new();
        for name in &entries {
            let mut prefix = String::new();
            for part in name.split('/') {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(part);
                let seen = folded
                    .entry(prefix.to_lowercase())
                    .or_insert_with(|| prefix.clone());
                if *seen != prefix {
                    return Err(format!(
                        "paths differing only by case in the bundle: {seen:?} and {prefix:?}"
                    ));
                }
            }
        }
        let out = temp("loj-tree")?;
        // Registered before writing, so a failure part-way still removes the tree.
        self.exports.borrow_mut().push(out.clone());
        for name in entries {
            let bytes = self
                .blob(commit, &name)
                .ok_or_else(|| format!("cannot read {name}"))?;
            let path = out.join(&name);
            if let Some(p) = path.parent() {
                std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        }
        Ok(out)
    }
}

impl Drop for Bundle {
    fn drop(&mut self) {
        for d in self.exports.borrow().iter() {
            let _ = std::fs::remove_dir_all(d);
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_names() {
        for ok in [
            "history.yaml",
            "versions/98mj5vgx/law.yaml",
            "a.b/c..d",
            ".gitattributes",
        ] {
            assert!(safe_name(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "/etc/passwd",
            "..",
            "../x",
            "a/../../x",
            "./x",
            "a//b",
            "a/",
            ".git/config",
            "a/.GIT/hooks/x",
            ".git./config",
            ".git /config",
            "GIT~1/config",
            "a/law.yaml.",
            "C:/x",
            "a:b",
            r"a\b",
            r"..\x",
        ] {
            assert!(safe_name(bad).is_err(), "{bad}");
        }
    }

    /// A scratch repository with git commands, for building hostile bundles.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Scratch {
            let s = Scratch(temp("loj-test").unwrap());
            s.run(&["init", "--quiet", "-b", "main"], None);
            s
        }
        fn run(&self, args: &[&str], stdin: Option<&[u8]>) -> String {
            use std::io::Write;
            use std::process::Stdio;
            let mut cmd = Command::new("git");
            for (key, _) in std::env::vars_os() {
                if key.to_string_lossy().starts_with("GIT_") {
                    cmd.env_remove(key);
                }
            }
            let mut child = cmd
                .arg("-C")
                .arg(&self.0)
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", NULL_DEVICE)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(stdin.unwrap_or(b""))
                .unwrap();
            let out = child.wait_with_output().unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).unwrap().trim().to_owned()
        }
        /// Commit a tree made of `(mode, type, name)` entries holding `content`, and
        /// bundle `main`.
        fn bundle(&self, entries: &[(&str, &str, &str)]) -> PathBuf {
            let blob = self.run(&["hash-object", "-w", "--stdin"], Some(b"x\n"));
            let mut spec = String::new();
            for (mode, kind, name) in entries {
                spec.push_str(&format!("{mode} {kind} {blob}\t{name}\n"));
            }
            let tree = self.run(&["mktree"], Some(spec.as_bytes()));
            let commit = self.run(&["commit-tree", &tree, "-m", "t"], None);
            self.run(&["update-ref", "refs/heads/main", &commit], None);
            let path = self.0.join("t.bundle");
            self.run(&["bundle", "create", &path.to_string_lossy(), "main"], None);
            path
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The bundle is either refused when fetched (fsck) or its export fails; it never
    /// writes outside the export directory.
    fn refused(entries: &[(&str, &str, &str)]) {
        let s = Scratch::new();
        let path = s.bundle(entries);
        let Ok(b) = Bundle::open(&path) else { return };
        let head = b.branch("main").unwrap();
        assert!(b.export(&head).is_err(), "{entries:?}");
        assert!(b.exports.borrow().is_empty(), "nothing was written");
    }

    #[test]
    fn regular_tree_exports() {
        let s = Scratch::new();
        let path = s.bundle(&[("100644", "blob", "history.yaml")]);
        let b = Bundle::open(&path).unwrap();
        let out = b.export(&b.branch("main").unwrap()).unwrap();
        assert_eq!(std::fs::read(out.join("history.yaml")).unwrap(), b"x\n");
        drop(b);
        assert!(!out.exists(), "the export is removed with the bundle");
    }

    #[test]
    fn traversal_names_are_refused() {
        refused(&[("100644", "blob", "..")]);
        refused(&[("100644", "blob", ".git")]);
        refused(&[("100644", "blob", "a:b")]);
        refused(&[("100644", "blob", r"..\x")]);
    }

    #[test]
    fn symlinks_are_refused() {
        refused(&[("120000", "blob", "link")]);
    }

    #[test]
    fn case_collisions_are_refused() {
        refused(&[("100644", "blob", "A.yaml"), ("100644", "blob", "a.yaml")]);
    }

    #[test]
    fn option_like_revisions_are_refused() {
        for bad in ["", "-h", "--all", "--output=x", "a b", "a\nb"] {
            assert!(revision(bad).is_err(), "{bad:?}");
        }
        let s = Scratch::new();
        let b = Bundle::open(&s.bundle(&[("100644", "blob", "history.yaml")])).unwrap();
        let head = b.branch("main").unwrap();
        assert!(revision(&head).is_ok());
        assert_eq!(b.commit("--all"), None);
        assert!(!b.is_ancestor("--all", &head));
        assert_eq!(b.range("--all", &head), None);
        assert_eq!(b.message("--all"), None);
        assert!(b.export("--all").is_err());
    }
}
