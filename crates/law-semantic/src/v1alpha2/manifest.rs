use super::{manifest_validator, Context, Source, MAX_FILE_BYTES, MAX_TOTAL_BYTES, SCHEMA_VERSION};
use crate::common::budget::ReadBudget;
use crate::common::yaml::parse as yaml;
use serde_json::Value;
use std::{collections::HashSet, fs, path::Path};

type Documents = Vec<(String, String, String)>;

fn read(path: &Path, name: &str, ctx: &mut Context, budget: &mut ReadBudget) -> Option<String> {
    let bytes = match budget.read_file(path) {
        Ok(Some(v)) => v,
        Ok(None) => {
            ctx.incomplete = true;
            Vec::new()
        }
        Err(e) => {
            if name == "law.yaml" {
                ctx.incomplete = true;
            } else {
                ctx.missing = true;
            }
            ctx.diag("MAN-02", &e.to_string(), name, 0, 0);
            return None;
        }
    };
    if ctx.incomplete {
        ctx.diag(
            "BASIC-02",
            "Input byte budget exceeded (16 MiB/file, 128 MiB total)",
            name,
            0,
            0,
        );
        return None;
    }
    let source = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => {
            ctx.sources.push(Source {
                path: name.into(),
                content: Err(e.into_bytes()),
            });
            ctx.diag("BASIC-02", "Invalid UTF-8", name, 0, 0);
            ctx.missing = true;
            return None;
        }
    };
    ctx.sources.push(Source {
        path: name.into(),
        content: Ok(source.clone()),
    });
    if source.is_empty()
        || source.starts_with('\u{feff}')
        || source.contains('\0')
        || source
            .as_bytes()
            .iter()
            .enumerate()
            .any(|(i, b)| *b == b'\r' && source.as_bytes().get(i + 1) != Some(&b'\n'))
    {
        ctx.diag(
            "BASIC-02",
            "Empty, BOM, NUL or bare CR input",
            name,
            0,
            source.len(),
        );
        ctx.missing = true;
        return None;
    }
    Some(source)
}

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

pub(super) fn load(path: &Path, ctx: &mut Context) -> Option<(Value, Documents)> {
    let mut budget = ReadBudget::new(MAX_TOTAL_BYTES, MAX_FILE_BYTES);
    let source = read(path, "law.yaml", ctx, &mut budget)?;
    let manifest = match yaml(&source) {
        Ok(v) => v,
        Err(e) => {
            ctx.diag("MAN-01", &e, "law.yaml", 0, source.len());
            return None;
        }
    };
    fn oversized(v: &Value) -> bool {
        match v {
            Value::String(s) => s.len() > 4096,
            Value::Array(a) => a.iter().any(oversized),
            Value::Object(m) => m.iter().any(|(k, v)| k.len() > 4096 || oversized(v)),
            _ => false,
        }
    }
    if oversized(&manifest)
        || manifest["documents"]
            .as_array()
            .is_some_and(|a| a.len() > 4096)
    {
        ctx.incomplete = true;
        ctx.diag(
            "MAN-01",
            "Implementation limit: 4096 bytes per Manifest string, 4096 documents",
            "law.yaml",
            0,
            source.len(),
        );
        return None;
    }
    if manifest["schemaVersion"] != SCHEMA_VERSION {
        // VAL-04: an unknown or other version is unprocessable, never read as this one.
        ctx.incomplete = true;
        let message = if manifest["schemaVersion"] == "loj/v1alpha1" {
            "Unsupported schemaVersion loj/v1alpha1; check it with --profile v1alpha1"
        } else {
            "Unsupported schemaVersion"
        };
        ctx.diag("BASIC-01", message, "law.yaml", 0, source.len());
        ctx.diagnostics.last_mut().unwrap().1.code = "loj/v1alpha2/unsupported-version".into();
        return None;
    }
    let errors: Vec<_> = manifest_validator()
        .iter_errors(&manifest)
        .map(|e| e.to_string())
        .collect();
    if !errors.is_empty() {
        for e in errors {
            ctx.diag("MAN-01", &e, "law.yaml", 0, source.len());
        }
        return None;
    }
    if manifest["law"]["promulgatedAt"]
        .as_str()
        .is_some_and(|s| s.starts_with("0000-"))
    {
        ctx.diag(
            "MAN-01",
            "Gregorian civil dates have no year zero",
            "law.yaml",
            0,
            source.len(),
        );
    }
    let mut scopes = vec!["main".to_owned()];
    for (field, prefix) in [("supplements", "supplement"), ("appendices", "appendix")] {
        if let Some(a) = manifest[field].as_array() {
            for s in a {
                let key = format!("{prefix}/{}", s["key"].as_str().unwrap());
                if scopes.contains(&key) {
                    ctx.diag("MAN-01", "Duplicate scope key", "law.yaml", 0, source.len());
                }
                scopes.push(key);
            }
        }
    }
    let root = match path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()
    {
        Ok(p) => p,
        Err(e) => {
            ctx.incomplete = true;
            ctx.diag("MAN-02", &e.to_string(), "law.yaml", 0, source.len());
            return None;
        }
    };
    let mut seen = HashSet::new();
    let mut identities = HashSet::new();
    let mut used = HashSet::new();
    let mut last = 0;
    let mut documents = Vec::new();
    for document in manifest["documents"].as_array().unwrap() {
        let name = document["path"].as_str().unwrap();
        let parent = document["parent"].as_str().unwrap();
        if let Some(index) = scopes.iter().position(|s| s == parent) {
            if index < last {
                ctx.diag(
                    "MAN-03",
                    "Non-contiguous or out-of-order scope",
                    "law.yaml",
                    0,
                    source.len(),
                );
            }
            last = index;
            used.insert(index);
        } else {
            ctx.diag("MAN-03", "Undefined scope", "law.yaml", 0, source.len());
        }
        if !safe_path(name) || !seen.insert(name.to_ascii_lowercase()) {
            ctx.diag(
                "MAN-02",
                "Forbidden or duplicate path",
                "law.yaml",
                0,
                source.len(),
            );
            ctx.missing = true;
            continue;
        }
        let mut physical = root.clone();
        let mut invalid = false;
        for part in name.split('/') {
            physical.push(part);
            if fs::symlink_metadata(&physical).is_ok_and(|m| linked(&m)) {
                invalid = true;
                break;
            }
        }
        if invalid || physical.canonicalize().is_ok_and(|p| !p.starts_with(&root)) {
            ctx.diag(
                "MAN-02",
                "Symlink, junction or escaping path",
                "law.yaml",
                0,
                source.len(),
            );
            ctx.missing = true;
            continue;
        }
        if fs::metadata(&physical).is_ok_and(|m| !m.is_file()) {
            ctx.diag("MAN-02", "Not a regular file", "law.yaml", 0, source.len());
            ctx.missing = true;
            continue;
        }
        match same_file::Handle::from_path(&physical) {
            Ok(h) => {
                if !identities.insert(h) {
                    ctx.diag(
                        "MAN-02",
                        "Duplicate physical file",
                        "law.yaml",
                        0,
                        source.len(),
                    );
                    ctx.missing = true;
                    continue;
                }
            }
            Err(e) => {
                ctx.missing = true;
                ctx.diag("MAN-02", &e.to_string(), "law.yaml", 0, source.len());
                continue;
            }
        }
        if let Some(text) = read(&physical, name, ctx, &mut budget) {
            documents.push((name.into(), text, parent.into()));
        }
        if ctx.incomplete {
            break;
        }
    }
    if used.len() != scopes.len() {
        ctx.diag(
            "MAN-03",
            "Every declared scope requires documents",
            "law.yaml",
            0,
            source.len(),
        );
    }
    Some((manifest, documents))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_sources_consume_total_budget_before_retention() {
        let dir = std::env::temp_dir().join(format!("loj2-budget-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for bytes in [b"\xffabc".as_slice(), b"\xef\xbb\xbfa", b"a\0bc", b"a\rbc"] {
            let path = dir.join("input.md");
            fs::write(&path, bytes).unwrap();
            let mut budget = ReadBudget::new(7, 100);
            let mut ctx = Context::default();
            assert!(read(&path, "first.md", &mut ctx, &mut budget).is_none());
            assert_eq!(budget.remaining(), 3);
            assert_eq!(ctx.sources.len(), 1);
            assert!(!ctx.incomplete);
            assert!(read(&path, "second.md", &mut ctx, &mut budget).is_none());
            assert!(ctx.incomplete);
            assert_eq!(
                ctx.sources.len(),
                1,
                "over-budget bytes must not be retained"
            );
            assert_eq!(budget.remaining(), 3, "metadata rejects before reading");
        }
    }

    #[test]
    fn mixed_valid_and_invalid_sources_share_exact_budget() {
        let dir = std::env::temp_dir().join(format!("loj2-mixed-budget-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("input.md");
        let mut ctx = Context::default();
        let mut budget = ReadBudget::new(8, 4);
        fs::write(&path, b"abcd").unwrap();
        assert_eq!(
            read(&path, "valid.md", &mut ctx, &mut budget).as_deref(),
            Some("abcd")
        );
        fs::write(&path, b"\xef\xbb\xbfa").unwrap();
        assert!(read(&path, "bom.md", &mut ctx, &mut budget).is_none());
        assert!(
            !ctx.incomplete,
            "exact budget is allowed even for invalid input"
        );
        assert_eq!(budget.remaining(), 0);
        assert_eq!(ctx.sources.len(), 2);
        fs::write(&path, b"x").unwrap();
        assert!(read(&path, "overflow.md", &mut ctx, &mut budget).is_none());
        assert!(ctx.incomplete);
        assert_eq!(ctx.sources.len(), 2);
    }
}
