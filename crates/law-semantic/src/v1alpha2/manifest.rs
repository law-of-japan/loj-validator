use super::{manifest_validator, Context, Source, MAX_FILE_BYTES, MAX_TOTAL_BYTES, SCHEMA_VERSION};
use crate::common::budget::ReadBudget;
use crate::common::documents::DocumentPaths;
use crate::common::yaml::parse as yaml;
use serde_json::Value;
use std::{collections::HashSet, path::Path};

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

/// Whether a manifest string or key is longer than the implementation reads.
fn oversized(v: &Value) -> bool {
    match v {
        Value::String(s) => s.len() > 4096,
        Value::Array(a) => a.iter().any(oversized),
        Value::Object(m) => m.iter().any(|(k, v)| k.len() > 4096 || oversized(v)),
        _ => false,
    }
}

/// MAN-01 / BASIC-01: the manifest as YAML of this version that satisfies
/// its schema. `None` when the documents cannot be read from it.
fn parse(source: &str, ctx: &mut Context) -> Option<Value> {
    let manifest = match yaml(source) {
        Ok(v) => v,
        Err(e) => {
            ctx.diag("MAN-01", &e, "law.yaml", 0, source.len());
            return None;
        }
    };
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
        ctx.diag_with_code(
            "BASIC-01",
            "loj/v1alpha2/unsupported-version",
            message,
            "law.yaml",
            0..source.len(),
        );
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
    Some(manifest)
}

/// The scopes of the manifest in order: `main`, then `supplement/<key>` and
/// `appendix/<key>`. MAN-01: a key names one scope.
fn scopes(manifest: &Value, end: usize, ctx: &mut Context) -> Vec<String> {
    let mut scopes = vec!["main".to_owned()];
    for (field, prefix) in [("supplements", "supplement"), ("appendices", "appendix")] {
        if let Some(a) = manifest[field].as_array() {
            for s in a {
                let key = format!(
                    "{prefix}/{}",
                    s["key"].as_str().expect("a string by the manifest schema")
                );
                if scopes.contains(&key) {
                    ctx.diag("MAN-01", "Duplicate scope key", "law.yaml", 0, end);
                }
                scopes.push(key);
            }
        }
    }
    scopes
}

pub(super) fn load(path: &Path, ctx: &mut Context) -> Option<(Value, Documents)> {
    let mut budget = ReadBudget::new(MAX_TOTAL_BYTES, MAX_FILE_BYTES);
    let source = read(path, "law.yaml", ctx, &mut budget)?;
    let manifest = parse(&source, ctx)?;
    let scopes = scopes(&manifest, source.len(), ctx);
    let mut paths = match DocumentPaths::new(path) {
        Ok(p) => p,
        Err(e) => {
            ctx.incomplete = true;
            ctx.diag("MAN-02", &e.to_string(), "law.yaml", 0, source.len());
            return None;
        }
    };
    let mut used = HashSet::new();
    let mut last = 0;
    let mut documents = Vec::new();
    for document in manifest["documents"]
        .as_array()
        .expect("an array by the manifest schema")
    {
        let name = document["path"]
            .as_str()
            .expect("a string by the manifest schema");
        let parent = document["parent"]
            .as_str()
            .expect("a string by the manifest schema");
        // MAN-03: the documents of a scope are contiguous and in scope order.
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
        let physical = match paths.resolve(name) {
            Ok(p) => p,
            Err(refused) => {
                ctx.missing = true;
                ctx.diag("MAN-02", &refused.message, "law.yaml", 0, source.len());
                continue;
            }
        };
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
    use std::fs;

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
