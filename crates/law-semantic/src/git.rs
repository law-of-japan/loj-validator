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

/// Create a fresh, empty temporary directory, readable only by the owner on Unix.
/// Creation fails if the name is already taken, so a directory planted in the shared
/// temp location is never reused.
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
        #[cfg(unix)]
        let created = {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new().mode(0o700).create(&dir)
        };
        #[cfg(not(unix))]
        let created = std::fs::DirBuilder::new().create(&dir);
        match created {
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
/// a space, or the short name `GIT~1`), and control characters such as a newline.
fn safe_name(name: &str) -> Result<(), String> {
    let bad = || Err(format!("unsafe path in the bundle tree: {name:?}"));
    if name.is_empty()
        || name.contains(['\\', ':'])
        || name.starts_with('/')
        || name.chars().any(char::is_control)
    {
        return bad();
    }
    if name.split('/').any(|c| {
        c.is_empty()
            || c == "."
            || c == ".."
            || c.ends_with(['.', ' '])
            || c.eq_ignore_ascii_case(".git")
            || c.eq_ignore_ascii_case("git~1")
            || windows_reserved(c)
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

/// A Windows device name (`CON`, `NUL`, `COM1`, …), with or without an extension.
fn windows_reserved(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
}

const MAX_FILE: u64 = 16 * 1024 * 1024;
/// The largest bundle file fetched. The tree budget below bounds what is exported;
/// this bounds what the fetch can write, history included.
const MAX_BUNDLE: u64 = 1024 * 1024 * 1024;
const MAX_TOTAL: u64 = 128 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;

/// The most output kept from one git command (a blob is at most MAX_FILE).
const MAX_OUTPUT: u64 = 2 * MAX_TOTAL;

/// The longest one git command may run before it is killed.
const GIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

type Output = (std::process::ExitStatus, Vec<u8>, Vec<u8>);

/// Run `cmd`, reading its output on separate threads so a full pipe never blocks
/// it, and kill it if it runs longer than `limit`. Only the direct child is killed:
/// this assumes plain git commands, which (with hooks disabled) start no process
/// that would keep the pipes open after git exits.
fn run_with_timeout(mut cmd: Command, limit: std::time::Duration) -> Result<Output, String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git is not available: {e}"))?;
    let reader = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            // Keep at most MAX_OUTPUT bytes; drain the rest so git never blocks on
            // a full pipe, and report the overflow.
            let mut buf = Vec::new();
            let mut overflow = false;
            if let Some(mut p) = pipe {
                let _ = (&mut p).take(MAX_OUTPUT + 1).read_to_end(&mut buf);
                if buf.len() as u64 > MAX_OUTPUT {
                    buf.truncate(MAX_OUTPUT as usize);
                    overflow = true;
                    let _ = std::io::copy(&mut p, &mut std::io::sink());
                }
            }
            (buf, overflow)
        })
    };
    let out = reader(child.stdout.take().map(|p| Box::new(p) as _));
    let err = reader(child.stderr.take().map(|p| Box::new(p) as _));
    let start = std::time::Instant::now();
    let mut pause = std::time::Duration::from_millis(1);
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if start.elapsed() > limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("git did not finish within {} s", limit.as_secs()));
            }
            None => {
                std::thread::sleep(pause);
                pause = (pause * 2).min(std::time::Duration::from_millis(50));
            }
        }
    };
    let (stdout, overflow) = out.join().unwrap_or_default();
    let (stderr, _) = err.join().unwrap_or_default();
    if overflow {
        return Err(format!("git output is larger than {MAX_OUTPUT} bytes"));
    }
    Ok((status, stdout, stderr))
}

/// Why a git command that ran to the end failed.
/// Only the first line of stderr, cut to `MAX_REASON` characters and with control
/// characters (terminal escapes, `\r`) and invisible format characters (bidi
/// overrides, zero-width marks) replaced by `?`, goes into the report: stderr can
/// be long, and a bundle chooses part of it.
fn failure(status: &std::process::ExitStatus, stderr: &[u8]) -> String {
    const MAX_REASON: usize = 300;
    let stderr = String::from_utf8_lossy(stderr);
    let first = stderr.trim().lines().next().unwrap_or("");
    let mut stderr: String = first
        .chars()
        .take(MAX_REASON)
        .map(|c| {
            if c.is_control() || invisible(c) {
                '?'
            } else {
                c
            }
        })
        .collect();
    if first.chars().count() > MAX_REASON {
        stderr.push('…');
    }
    if stderr.is_empty() {
        format!("git failed ({status})")
    } else {
        format!("git failed ({status}): {stderr}")
    }
}

/// A format character that changes how text around it is shown without being
/// seen itself: zero-width marks, bidi embeddings, overrides and isolates, and
/// the byte order mark.
fn invisible(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}')
}

#[cfg(windows)]
const NULL_DEVICE: &str = "NUL";
#[cfg(not(windows))]
const NULL_DEVICE: &str = "/dev/null";

/// A revision argument from the caller (CLI, reference string, registry). Only a
/// commit hash (4 to 64 lowercase hex digits) or `refs/heads/<branch>` is accepted,
/// so no value can act as an option, a range or a rev expression (`^`, `~`, `:`,
/// `@{`, …).
fn revision(rev: &str) -> Result<(), String> {
    let hash = (4..=64).contains(&rev.len())
        && rev.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    let branch = rev.strip_prefix("refs/heads/").is_some_and(|name| {
        !name.is_empty()
            && !name.starts_with(['-', '/', '.'])
            && !name.ends_with(['/', '.'])
            && !name.contains("..")
            && !name.contains("//")
            && !name.ends_with(".lock")
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
    });
    if hash || branch {
        Ok(())
    } else {
        Err(format!("not a commit hash or branch: {rev:?}"))
    }
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
        let size = std::fs::metadata(&bundle)
            .map_err(|e| format!("bundle unreadable: {e}"))?
            .len();
        if size > MAX_BUNDLE {
            return Err(format!("bundle is larger than {MAX_BUNDLE} bytes"));
        }
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
    /// Run a git command that must succeed: its stdout, or `Err` when git failed.
    fn git(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        let (status, stdout, stderr) = self.run(args)?;
        if !status.success() {
            return Err(failure(&status, &stderr));
        }
        Ok(stdout)
    }

    /// Run a git command whose exit status 1 means "no" (the object is absent, the
    /// commit is not an ancestor): `Ok(Some(stdout))` on 0, `Ok(None)` on 1, and
    /// `Err` when git failed (timeout, output limit, any other status). Use it only
    /// for such commands (`rev-parse --verify --quiet`, `merge-base --is-ancestor`):
    /// for most git commands, 1 is a failure.
    fn probe(&self, args: &[&str]) -> Result<Option<Vec<u8>>, String> {
        let (status, stdout, stderr) = self.run(args)?;
        match status.code() {
            Some(0) => Ok(Some(stdout)),
            Some(1) => Ok(None),
            // Any other status, or none (git was killed by a signal).
            _ => Err(failure(&status, &stderr)),
        }
    }

    /// Run git in the bundle's repository, isolated from the caller's git
    /// environment (`GIT_DIR`, `GIT_CONFIG_*`, …) and from the user and system
    /// configuration, so the result depends only on the bundle. `Err` means git
    /// did not run to the end (not started, timed out, output over the limit).
    fn run(&self, args: &[&str]) -> Result<Output, String> {
        let mut cmd = Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                cmd.env_remove(key);
            }
        }
        cmd.arg("-C")
            .arg(&self.dir)
            .args(["-c", "core.autocrlf=false", "-c", "core.safecrlf=false"])
            .args(["-c", &format!("core.hooksPath={NULL_DEVICE}")])
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", NULL_DEVICE)
            .env("GIT_TERMINAL_PROMPT", "0");
        run_with_timeout(cmd, GIT_TIMEOUT)
    }
    /// The full hash of a commit. `Ok(None)` means the revision is not a commit
    /// hash or branch, or no such commit is in the bundle; `Err` means git failed.
    pub fn commit(&self, rev: &str) -> Result<Option<String>, String> {
        if revision(rev).is_err() {
            return Ok(None);
        }
        let out = self.probe(&[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{rev}^{{commit}}"),
        ])?;
        out.map(|b| {
            String::from_utf8(b)
                .map(|s| s.trim().to_owned())
                .map_err(|_| "git printed a non-UTF-8 commit hash".to_owned())
        })
        .transpose()
    }
    pub fn branch(&self, name: &str) -> Result<Option<String>, String> {
        self.commit(&format!("refs/heads/{name}"))
    }
    /// Whether `a` is an ancestor of `b`. Both must be resolved commits (callers
    /// pass the hashes `commit` returned): an invalid revision is an `Err`, never
    /// "not an ancestor", and so is a git failure or a commit missing from the
    /// bundle.
    pub fn is_ancestor(&self, a: &str, b: &str) -> Result<bool, String> {
        revision(a)?;
        revision(b)?;
        Ok(self
            .probe(&["merge-base", "--is-ancestor", a, b])?
            .is_some())
    }
    /// Commits of `base..head`, oldest first, each with its first parent. Both must
    /// be resolved commits (callers pass the hashes `commit` returned); an invalid
    /// revision is an `Err`, like a git failure.
    pub fn range(&self, base: &str, head: &str) -> Result<Vec<(String, Option<String>)>, String> {
        revision(base)?;
        revision(head)?;
        let list = self.git(&[
            "rev-list",
            "--reverse",
            "--parents",
            &format!("{base}..{head}"),
        ])?;
        let list = String::from_utf8(list)
            .map_err(|_| "git printed a non-UTF-8 commit list".to_owned())?;
        Ok(list
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| {
                let mut w = l.split(' ');
                let c = w.next().unwrap_or("").to_owned();
                (c, w.next().map(str::to_owned))
            })
            .collect())
    }
    /// The raw commit message (UTF-8, CHG-03). The header ends at the first empty
    /// line; continuation lines of multi-line headers (e.g. `gpgsig`) start with a space.
    /// `commit` must exist. `Ok(None)` means the commit object is not UTF-8; `Err`
    /// means git failed.
    pub fn message(&self, commit: &str) -> Result<Option<String>, String> {
        revision(commit)?;
        let raw = self.git(&["cat-file", "commit", commit])?;
        let Ok(raw) = String::from_utf8(raw) else {
            return Ok(None);
        };
        // A commit object with no blank line after the header has no message;
        // like an empty message, it declares no trailer (CHG-03).
        Ok(Some(
            raw.split_once("\n\n")
                .map(|(_, m)| m.to_owned())
                .unwrap_or_default(),
        ))
    }
    /// The blob at `commit:path`, raw, for an entry listed in the tree. `Err`
    /// means git failed.
    fn blob(&self, commit: &str, path: &str) -> Result<Vec<u8>, String> {
        revision(commit)?;
        self.git(&["cat-file", "blob", &format!("{commit}:{path}")])
            .map_err(|e| format!("cannot read {path}: {e}"))
    }
    /// Write the tree of `commit` to a fresh directory, byte for byte. Every entry
    /// name is checked before anything is written; symbolic links, submodules and
    /// non-UTF-8 names are refused with an `Err`, never followed or written.
    pub fn export(&self, commit: &str) -> Result<PathBuf, ExportError> {
        use ExportError::{Environment, Tree};
        // Callers pass resolved hashes; an invalid one is a caller bug, which must
        // stop the check like an environment failure rather than be noted as a
        // property of the tree.
        revision(commit).map_err(Environment)?;
        let listing = self
            .git(&["ls-tree", "-r", "-z", "-l", "--full-tree", commit])
            .map_err(Environment)?;
        let mut entries = Vec::new();
        let mut total: u64 = 0;
        for entry in listing.split(|b| *b == 0).filter(|n| !n.is_empty()) {
            let entry = String::from_utf8(entry.to_vec())
                .map_err(|_| {
                    format!(
                        "non-UTF-8 path in the bundle tree: {:?}",
                        String::from_utf8_lossy(entry)
                    )
                })
                .map_err(Tree)?;
            let (meta, name) = entry
                .split_once('\t')
                .ok_or_else(|| format!("unexpected ls-tree entry: {entry:?}"))
                .map_err(Tree)?;
            let mode = meta.split(' ').next().unwrap_or("");
            safe_name(name).map_err(Tree)?;
            if !matches!(mode, "100644" | "100755") {
                return Err(Tree(format!(
                    "unsupported tree entry (mode {mode}) in the bundle: {name:?}"
                )));
            }
            // The same budget as a version package (16 MiB per file, 128 MiB in
            // all), checked from the tree before anything is read or written.
            let size: u64 = meta
                .split_whitespace()
                .last()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| format!("unexpected ls-tree entry: {entry:?}"))
                .map_err(Tree)?;
            total = total.saturating_add(size);
            if size > MAX_FILE || total > MAX_TOTAL {
                return Err(Tree(format!(
                    "bundle tree exceeds the input budget (16 MiB/file, 128 MiB total) at {name:?}"
                )));
            }
            if entries.len() >= MAX_ENTRIES {
                return Err(Tree(format!(
                    "bundle tree has more than {MAX_ENTRIES} files"
                )));
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
                    return Err(Tree(format!(
                        "paths differing only by case in the bundle: {seen:?} and {prefix:?}"
                    )));
                }
            }
        }
        let out = temp("loj-tree").map_err(Environment)?;
        // Registered before writing, so a failure part-way still removes the tree.
        self.exports.borrow_mut().push(out.clone());
        for name in entries {
            let bytes = self.blob(commit, &name).map_err(Environment)?;
            let path = out.join(&name);
            if let Some(p) = path.parent() {
                std::fs::create_dir_all(p).map_err(|e| Environment(e.to_string()))?;
            }
            std::fs::write(&path, bytes).map_err(|e| Environment(e.to_string()))?;
        }
        Ok(out)
    }
}

/// Why a tree could not be exported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    /// The tree itself is refused (an unsafe or non-UTF-8 name, an unsupported
    /// entry, the input budget, a case collision): a property of that commit.
    Tree(String),
    /// Git failed, or the export could not be written: a property of the
    /// environment, which other commits would hit too.
    Environment(String),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::Tree(e) | ExportError::Environment(e) => f.write_str(e),
        }
    }
}

#[cfg(test)]
impl Bundle {
    /// Break the repository so that every later git command fails with status
    /// 128 ("bad config line"). Unlike removing `HEAD`, this does not depend on
    /// git's repository discovery or on where the temp directory lies.
    pub(crate) fn break_for_test(&self) {
        std::fs::write(
            self.dir.join("config"),
            "[[[bad
",
        )
        .unwrap();
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
pub(crate) mod tests {
    use super::*;

    #[test]
    fn safe_names() {
        for ok in [
            "history.yaml",
            "versions/98mj5vgx/law.yaml",
            "a.b/c..d",
            ".gitattributes",
            "console.md",
            "com0.md",
            "COM10",
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
            "a\nb",
            "a\tb",
            "a\u{7f}b",
            "CON",
            "a/nul.txt",
            "com1.md",
            "LPT9",
        ] {
            assert!(safe_name(bad).is_err(), "{bad}");
        }
    }

    /// A scratch repository with git commands, for building hostile bundles.
    pub(crate) struct Scratch(PathBuf);
    impl Scratch {
        pub(crate) fn new() -> Scratch {
            let s = Scratch(temp("loj-test").unwrap());
            s.run(&["init", "--quiet", "-b", "main"], None);
            s
        }
        pub(crate) fn run(&self, args: &[&str], stdin: Option<&[u8]>) -> String {
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
        /// Commit a tree made of `(mode, type, name)` entries holding `x\n`, and
        /// bundle `main`.
        pub(crate) fn bundle(&self, entries: &[(&str, &str, &str)]) -> PathBuf {
            self.bundle_with(b"x\n", entries)
        }
        fn bundle_with(&self, content: &[u8], entries: &[(&str, &str, &str)]) -> PathBuf {
            let blob = self.run(&["hash-object", "-w", "--stdin"], Some(content));
            let mut spec = String::new();
            for (mode, kind, name) in entries {
                spec.push_str(&format!("{mode} {kind} {blob}\t{name}\n"));
            }
            let tree = self.run(&["mktree"], Some(spec.as_bytes()));
            self.bundle_tree(&tree)
        }
        /// Commit `tree` (a tree hash) on `main`, and bundle `main`.
        pub(crate) fn bundle_tree(&self, tree: &str) -> PathBuf {
            let commit = self.run(&["commit-tree", tree, "-m", "t"], None);
            self.bundle_head(&commit)
        }
        /// Point `main` at `commit`, and bundle `main`.
        pub(crate) fn bundle_head(&self, commit: &str) -> PathBuf {
            self.run(&["update-ref", "refs/heads/main", commit], None);
            let path = self.0.join("t.bundle");
            self.run(&["bundle", "create", &path.to_string_lossy(), "main"], None);
            path
        }
        /// A chain of commits on `main`, one per message, all with the same tree
        /// holding an empty `history.yaml`; bundle `main` and return the bundle and
        /// the commits, oldest first. The commit objects are written byte for byte,
        /// so a message need not be UTF-8 (`commit-tree` would re-encode it).
        pub(crate) fn chain(&self, messages: &[&[u8]]) -> (PathBuf, Vec<String>) {
            let blob = self.run(&["hash-object", "-w", "--stdin"], Some(b""));
            let spec = format!("100644 blob {blob}\thistory.yaml\n");
            let tree = self.run(&["mktree"], Some(spec.as_bytes()));
            let mut commits: Vec<String> = Vec::new();
            for message in messages {
                let mut raw = format!("tree {tree}\n").into_bytes();
                if let Some(parent) = commits.last() {
                    raw.extend(format!("parent {parent}\n").bytes());
                }
                raw.extend(b"author t <t@example.invalid> 0 +0000\n");
                raw.extend(b"committer t <t@example.invalid> 0 +0000\n\n");
                raw.extend(*message);
                let c = self.run(
                    &["hash-object", "-t", "commit", "-w", "--stdin"],
                    Some(&raw),
                );
                commits.push(c);
            }
            let head = commits.last().expect("at least one message");
            self.run(&["update-ref", "refs/heads/main", head], None);
            let path = self.0.join("t.bundle");
            self.run(&["bundle", "create", &path.to_string_lossy(), "main"], None);
            (path, commits)
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
        let head = b.branch("main").unwrap().unwrap();
        assert!(b.export(&head).is_err(), "{entries:?}");
        assert!(b.exports.borrow().is_empty(), "nothing was written");
    }

    #[test]
    fn regular_tree_exports() {
        let s = Scratch::new();
        let path = s.bundle(&[("100644", "blob", "history.yaml")]);
        let b = Bundle::open(&path).unwrap();
        let out = b.export(&b.branch("main").unwrap().unwrap()).unwrap();
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
    fn oversized_files_are_refused() {
        let s = Scratch::new();
        let big = vec![b'x'; MAX_FILE as usize + 1];
        let b = Bundle::open(&s.bundle_with(&big, &[("100644", "blob", "law.yaml")])).unwrap();
        let err = b
            .export(&b.branch("main").unwrap().unwrap())
            .unwrap_err()
            .to_string();
        assert!(err.contains("input budget"), "{err}");
        assert!(b.exports.borrow().is_empty(), "nothing was written");
        // Nine 16 MiB files exceed the 128 MiB total.
        let s = Scratch::new();
        let full = vec![b'x'; MAX_FILE as usize];
        let names: Vec<String> = (0..9).map(|i| format!("f{i}.md")).collect();
        let entries: Vec<_> = names
            .iter()
            .map(|n| ("100644", "blob", n.as_str()))
            .collect();
        let b = Bundle::open(&s.bundle_with(&full, &entries)).unwrap();
        assert!(b.export(&b.branch("main").unwrap().unwrap()).is_err());
    }

    #[test]
    fn slow_commands_are_killed() {
        #[cfg(windows)]
        let cmd = {
            let mut c = Command::new("ping");
            c.args(["-n", "30", "127.0.0.1"]);
            c
        };
        #[cfg(not(windows))]
        let cmd = {
            let mut c = Command::new("sleep");
            c.arg("30");
            c
        };
        let start = std::time::Instant::now();
        let err = run_with_timeout(cmd, std::time::Duration::from_millis(300)).unwrap_err();
        assert!(err.contains("did not finish"), "{err}");
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
    }

    #[test]
    fn option_like_revisions_are_refused() {
        for bad in [
            "",
            "-h",
            "--all",
            "--output=x",
            "a b",
            "a\nb",
            "a..b",
            "a...b",
            "HEAD",
            "abc",
            "ABCDEF12",
            "abcd^",
            "abcd~1",
            "abcd:history.yaml",
            "main@{1}",
            "refs/heads/",
            "refs/heads/-x",
            "refs/heads/a..b",
            "refs/heads/a^",
            "refs/heads/x.lock",
            "refs/tags/v1",
        ] {
            assert!(revision(bad).is_err(), "{bad:?}");
        }
        for ok in [
            "abcd",
            "0123456789abcdef0123456789abcdef01234567",
            "refs/heads/main",
            "refs/heads/feat/x-1",
        ] {
            assert!(revision(ok).is_ok(), "{ok:?}");
        }
        let s = Scratch::new();
        let b = Bundle::open(&s.bundle(&[("100644", "blob", "history.yaml")])).unwrap();
        let head = b.branch("main").unwrap().unwrap();
        assert!(revision(&head).is_ok());
        assert_eq!(b.commit("--all"), Ok(None));
        assert!(b.is_ancestor("--all", &head).is_err());
        assert!(b.range("--all", &head).is_err());
        assert!(b.message("--all").is_err());
        assert!(b.export("--all").is_err());
    }

    /// A commit object that ends after its header (no blank line) has an empty
    /// message, which the change check reports as declaring no `LoJ-Change`.
    #[test]
    fn commit_without_blank_line_has_empty_message() {
        let s = Scratch::new();
        let tree = s.run(&["mktree"], Some(b""));
        let raw = format!(
            "tree {tree}\nauthor t <t@example.invalid> 0 +0000\ncommitter t <t@example.invalid> 0 +0000\n"
        );
        let c = s.run(
            &["hash-object", "-t", "commit", "-w", "--stdin"],
            Some(raw.as_bytes()),
        );
        s.run(&["update-ref", "refs/heads/main", &c], None);
        let path = s.0.join("t.bundle");
        s.run(&["bundle", "create", &path.to_string_lossy(), "main"], None);
        let b = Bundle::open(&path).unwrap();
        assert_eq!(b.message(&c), Ok(Some(String::new())));
    }

    /// A refused tree is a `Tree` error; a git failure while exporting is an
    /// `Environment` error, so the change check can stop on it.
    #[test]
    fn export_errors_say_whether_git_failed() {
        let s = Scratch::new();
        let b = Bundle::open(&s.bundle(&[("100644", "blob", "a:b")])).unwrap();
        let head = b.branch("main").unwrap().unwrap();
        assert!(matches!(b.export(&head), Err(ExportError::Tree(_))));
        let s = Scratch::new();
        let b = Bundle::open(&s.bundle(&[("100644", "blob", "history.yaml")])).unwrap();
        let head = b.branch("main").unwrap().unwrap();
        b.break_for_test();
        assert!(matches!(
            b.export(&head),
            Err(ExportError::Environment(e)) if e.starts_with("git failed")
        ));
    }

    /// The reason is one line, bounded, and free of control characters.
    #[test]
    fn failure_reasons_are_bounded_and_clean() {
        #[cfg(windows)]
        let status = {
            use std::os::windows::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(128)
        };
        #[cfg(unix)]
        let status = {
            use std::os::unix::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(128 << 8)
        };
        let reason = failure(&status, b"fatal: \x1b[31mred\x1b[0m\rx\nsecond line");
        assert!(reason.ends_with("fatal: ?[31mred?[0m?x"), "{reason}");
        let reason = failure(&status, "a\u{202E}b\u{200B}c".as_bytes());
        assert!(reason.ends_with("a?b?c"), "{reason}");
        let long = "x".repeat(1000);
        let reason = failure(&status, long.as_bytes());
        assert!(
            reason.ends_with('…') && reason.chars().count() < 400,
            "{reason}"
        );
    }

    /// `probe` maps exit status 0 and 1 of `merge-base --is-ancestor` to yes and no.
    #[test]
    fn ancestry_is_yes_or_no() {
        let s = Scratch::new();
        let (path, c) = s.chain(&[b"a\n", b"b\n"]);
        let b = Bundle::open(&path).unwrap();
        assert_eq!(b.is_ancestor(&c[0], &c[1]), Ok(true));
        assert_eq!(b.is_ancestor(&c[1], &c[0]), Ok(false));
    }

    /// An absent object is `Ok(None)`; a git failure is `Err`, never "absent".
    #[test]
    fn git_failures_are_not_absence() {
        let s = Scratch::new();
        let b = Bundle::open(&s.bundle(&[("100644", "blob", "history.yaml")])).unwrap();
        let head = b.branch("main").unwrap().unwrap();
        let absent = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(b.commit(absent), Ok(None));
        assert_eq!(b.branch("nope"), Ok(None));
        assert_eq!(b.is_ancestor(&head, &head), Ok(true));
        // A missing commit makes merge-base and cat-file fail (status 128).
        assert!(b.is_ancestor(absent, &head).is_err());
        assert!(b.message(absent).is_err());
        let msg = b.message(&head).unwrap().unwrap();
        assert_eq!(msg.trim(), "t");
        // With the object store gone, reading the message fails instead of
        // yielding an empty message.
        std::fs::remove_dir_all(b.dir.join("objects")).unwrap();
        let err = b.message(&head).unwrap_err();
        assert!(err.starts_with("git failed"), "{err}");
        assert!(b.range(&head, &head).is_err());
    }
}
