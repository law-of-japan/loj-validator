//! MAN-02: the document paths of a manifest, resolved to regular files below
//! the directory of the manifest. Shared by loj/v1alpha1 and loj/v1alpha2.
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

fn safe_path(path: &str) -> bool {
    path.ends_with(".md")
        && path.split('/').all(|s| {
            let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
            !s.is_empty()
                && s != "."
                && s != ".."
                && !s.ends_with('.')
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
                && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                && !(stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        })
}
fn linked(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

/// Why a document path was refused.
pub(crate) struct Refused {
    pub(crate) message: String,
    /// The file system failed while the path was examined; nothing is known
    /// about the path itself.
    pub(crate) io: bool,
}

fn refused(message: &str) -> Refused {
    Refused {
        message: message.into(),
        io: false,
    }
}

/// The documents of one manifest that have been resolved so far.
pub(crate) struct DocumentPaths {
    root: PathBuf,
    /// Paths as written, in ASCII lower case.
    seen: HashSet<String>,
    /// The files themselves, so that two names of one file are found.
    identities: HashSet<same_file::Handle>,
}

impl DocumentPaths {
    /// `manifest` is the path of `law.yaml`; documents are below its directory.
    pub(crate) fn new(manifest: &Path) -> std::io::Result<Self> {
        let root = manifest
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()?;
        Ok(DocumentPaths {
            root,
            seen: HashSet::new(),
            identities: HashSet::new(),
        })
    }

    /// The file that the document path `name` names. It is refused when the
    /// path is not portable, repeats an earlier one, passes through a link,
    /// leaves the directory, is not a regular file, or is a file that an
    /// earlier path already named.
    pub(crate) fn resolve(&mut self, name: &str) -> Result<PathBuf, Refused> {
        if !safe_path(name) || !self.seen.insert(name.to_ascii_lowercase()) {
            return Err(refused("Forbidden or duplicate path"));
        }
        let mut physical = self.root.clone();
        let mut invalid = false;
        for part in name.split('/') {
            physical.push(part);
            if fs::symlink_metadata(&physical).is_ok_and(|m| linked(&m)) {
                invalid = true;
                break;
            }
        }
        if invalid
            || physical
                .canonicalize()
                .is_ok_and(|p| !p.starts_with(&self.root))
        {
            return Err(refused("Symlink, junction or escaping path"));
        }
        if fs::metadata(&physical).is_ok_and(|m| !m.is_file()) {
            return Err(refused("Not a regular file"));
        }
        match same_file::Handle::from_path(&physical) {
            Ok(h) => {
                if !self.identities.insert(h) {
                    return Err(refused("Duplicate physical file"));
                }
            }
            Err(e) => {
                return Err(Refused {
                    message: e.to_string(),
                    io: true,
                })
            }
        }
        Ok(physical)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_portable_markdown_paths_are_safe() {
        for ok in [
            "main.md",
            "a/b-1/c_2.md",
            "A.B.md",
            "com0.md",
            "lpt.md",
            "console.md",
        ] {
            assert!(safe_path(ok), "{ok}");
        }
        for bad in [
            "main.txt",
            "main",
            "",
            "/main.md",
            "a//b.md",
            "./a.md",
            "../a.md",
            "a/../b.md",
            "a./b.md",
            "a\\b.md",
            "a b.md",
            "本則.md",
            "con.md",
            "a/NUL.md",
            "aux.x/a.md",
            "COM1.md",
            "lpt9/a.md",
        ] {
            assert!(!safe_path(bad), "{bad}");
        }
    }

    #[test]
    fn documents_resolve_once_and_only_to_regular_files_below_the_root() {
        let dir = std::env::temp_dir().join(format!("loj-document-paths-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("law.yaml"), "x").unwrap();
        fs::write(dir.join("main.md"), "x").unwrap();
        fs::write(dir.join("sub/a.md"), "x").unwrap();
        fs::create_dir_all(dir.join("dir.md")).unwrap();
        let mut paths = DocumentPaths::new(&dir.join("law.yaml")).unwrap();
        let message = |r: Result<PathBuf, Refused>| r.err().map(|e| (e.message, e.io));
        assert!(paths.resolve("main.md").is_ok_and(|p| p.is_file()));
        assert!(paths.resolve("sub/a.md").is_ok());
        // The same path again, also in another case.
        for again in ["main.md", "MAIN.md", "Sub/A.md"] {
            assert_eq!(
                message(paths.resolve(again)),
                Some(("Forbidden or duplicate path".to_owned(), false)),
                "{again}"
            );
        }
        assert_eq!(
            message(paths.resolve("../x.md")),
            Some(("Forbidden or duplicate path".to_owned(), false))
        );
        assert_eq!(
            message(paths.resolve("dir.md")),
            Some(("Not a regular file".to_owned(), false))
        );
        // A file that does not exist cannot be opened: the file system says why.
        assert!(message(paths.resolve("missing.md")).is_some_and(|(_, io)| io));
        fs::remove_dir_all(&dir).unwrap();
        assert!(DocumentPaths::new(&dir.join("law.yaml")).is_err());
    }
}
