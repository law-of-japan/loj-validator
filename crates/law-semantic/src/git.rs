//! Minimal read-only access to a `git bundle` through the `git` command.
//! Blobs are read raw (no line-ending conversion), so byte positions hold.
use std::path::{Path, PathBuf};
use std::process::Command;

/// A bare repository fetched from a bundle into a temporary directory.
pub struct Bundle {
    dir: PathBuf,
    exports: std::cell::RefCell<Vec<PathBuf>>,
}

fn temp(prefix: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{nanos}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ))
}

impl Bundle {
    /// Fetch every branch of the bundle. `Err` means the input is not available.
    pub fn open(bundle: &Path) -> Result<Bundle, String> {
        let bundle = bundle
            .canonicalize()
            .map_err(|e| format!("bundle unreadable: {e}"))?;
        // A verbatim `\?\` prefix would be read by git as a host name.
        let text = bundle.to_string_lossy().into_owned();
        let bundle = PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text));
        let dir = temp("loj-bundle");
        let b = Bundle {
            dir,
            exports: Default::default(),
        };
        b.git(&["init", "--quiet", "--bare", "."], true)?;
        b.git(
            &[
                "fetch",
                "--quiet",
                &bundle.to_string_lossy(),
                "+refs/heads/*:refs/heads/*",
            ],
            false,
        )?;
        Ok(b)
    }
    fn git(&self, args: &[&str], create: bool) -> Result<Vec<u8>, String> {
        if create {
            std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        }
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.dir)
            .args(["-c", "core.autocrlf=false", "-c", "core.safecrlf=false"])
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|e| format!("git is not available: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
        }
        Ok(out.stdout)
    }
    fn text(&self, args: &[&str]) -> Option<String> {
        self.git(args, false)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .map(|s| s.trim().to_owned())
    }
    /// The full hash of an existing commit, if the object is present.
    pub fn commit(&self, rev: &str) -> Option<String> {
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
        self.git(&["merge-base", "--is-ancestor", a, b], false)
            .is_ok()
    }
    /// Commits of `base..head`, oldest first, each with its first parent.
    pub fn range(&self, base: &str, head: &str) -> Option<Vec<(String, Option<String>)>> {
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
    /// The raw commit message (UTF-8, CHG-03).
    pub fn message(&self, commit: &str) -> Option<String> {
        let raw = self.git(&["cat-file", "commit", commit], false).ok()?;
        let raw = String::from_utf8(raw).ok()?;
        raw.split_once("\n\n").map(|(_, m)| m.to_owned())
    }
    /// A blob at `commit:path`, raw.
    pub fn blob(&self, commit: &str, path: &str) -> Option<Vec<u8>> {
        self.git(&["cat-file", "blob", &format!("{commit}:{path}")], false)
            .ok()
    }
    /// Write the tree of `commit` to a fresh directory, byte for byte.
    pub fn export(&self, commit: &str) -> Result<PathBuf, String> {
        let names = self.git(&["ls-tree", "-r", "-z", "--name-only", commit], false)?;
        let out = temp("loj-tree");
        for name in names.split(|b| *b == 0).filter(|n| !n.is_empty()) {
            let name = String::from_utf8(name.to_vec()).map_err(|e| e.to_string())?;
            let bytes = self
                .blob(commit, &name)
                .ok_or_else(|| format!("cannot read {name}"))?;
            let path = out.join(&name);
            if let Some(p) = path.parent() {
                std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        }
        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        self.exports.borrow_mut().push(out.clone());
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
