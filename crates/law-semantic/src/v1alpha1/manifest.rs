use super::{Context, MANIFEST_SCHEMA, MAX_FILE_BYTES, MAX_TOTAL_BYTES};
use serde_json::{json, Map, Value};
use std::{collections::HashSet, fs, io::Read, path::Path};
use yaml_rust2::{
    parser::{Event, Parser},
    scanner::{Scanner, TScalarStyle, TokenType},
};

type Documents = Vec<(String, String, String)>;

fn yaml(source: &str) -> Result<Value, String> {
    for token in Scanner::new(source.chars()) {
        if matches!(token.1,TokenType::VersionDirective(major,minor) if (major,minor)!=(1,2)) {
            return Err("Only YAML 1.2 is supported".into());
        }
    }
    let mut parser = Parser::new_from_str(source);
    fn next(parser: &mut Parser<std::str::Chars<'_>>) -> Result<Event, String> {
        parser.next_token().map(|v| v.0).map_err(|e| e.to_string())
    }
    fn value(
        parser: &mut Parser<std::str::Chars<'_>>,
        event: Event,
        depth: usize,
    ) -> Result<Value, String> {
        if depth > 32 {
            return Err("YAML nesting exceeds 32".into());
        }
        match event {
            Event::Scalar(s, style, 0, None) => {
                if style != TScalarStyle::Plain {
                    return Ok(Value::String(s));
                }
                if s.is_empty() || s == "~" {
                    return Err("Use explicit JSON null".into());
                }
                if let Ok(v) = serde_json::from_str::<Value>(&s) {
                    if !v.is_array() && !v.is_object() {
                        return Ok(v);
                    }
                }
                if s == "<<"
                    || non_json_scalar(&s)
                    || matches!(
                        s.as_str(),
                        "True"
                            | "False"
                            | "Null"
                            | "TRUE"
                            | "FALSE"
                            | "NULL"
                            | ".nan"
                            | ".inf"
                            | "-.inf"
                    )
                {
                    return Err("Non-JSON plain scalar; quote strings such as dates".into());
                }
                Ok(Value::String(s))
            }
            Event::SequenceStart(0, None) => {
                let mut a = Vec::new();
                loop {
                    let e = next(parser)?;
                    if e == Event::SequenceEnd {
                        break;
                    }
                    a.push(value(parser, e, depth + 1)?);
                }
                Ok(Value::Array(a))
            }
            Event::MappingStart(0, None) => {
                let mut map = Map::new();
                loop {
                    let e = next(parser)?;
                    if e == Event::MappingEnd {
                        break;
                    }
                    let key = value(parser, e, depth + 1)?
                        .as_str()
                        .ok_or("Mapping keys must be strings")?
                        .to_owned();
                    if key == "<<" || map.contains_key(&key) {
                        return Err("Duplicate or merge key".into());
                    }
                    let e = next(parser)?;
                    map.insert(key, value(parser, e, depth + 1)?);
                }
                Ok(Value::Object(map))
            }
            _ => Err("Tags, anchors, aliases and non-JSON YAML are forbidden".into()),
        }
    }
    if next(&mut parser)? != Event::StreamStart || next(&mut parser)? != Event::DocumentStart {
        return Err("Expected one YAML document".into());
    }
    let e = next(&mut parser)?;
    let v = value(&mut parser, e, 0)?;
    if next(&mut parser)? != Event::DocumentEnd || next(&mut parser)? != Event::StreamEnd {
        return Err("Multiple YAML documents".into());
    }
    Ok(v)
}

fn non_json_scalar(s: &str) -> bool {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| regex::Regex::new(r"^(?:[0-9]{4}-[0-9]{2}-[0-9]{2}(?:[Tt ].*)?|[-+]?(?:[0-9][0-9_.eE+-]*|\.[0-9][0-9eE+-]*|0[xXoObB][0-9a-fA-F_]+)|(?i:[-+]?\.(?:inf|nan)))$").unwrap()).is_match(s)
}

struct ReadBudget {
    remaining: usize,
    per_file: u64,
}

impl Default for ReadBudget {
    fn default() -> Self {
        Self {
            remaining: MAX_TOTAL_BYTES,
            per_file: MAX_FILE_BYTES,
        }
    }
}

impl ReadBudget {
    // One bounded lookahead byte detects growth after metadata inspection.
    // It is never retained; the caller stops loading on overflow.
    fn read(&mut self, reader: impl Read) -> std::io::Result<(Vec<u8>, bool)> {
        let limit = self.per_file.min(self.remaining as u64);
        let mut bytes = Vec::new();
        let result = reader.take(limit + 1).read_to_end(&mut bytes);
        self.remaining = self.remaining.saturating_sub(bytes.len());
        result?;
        let exceeded = bytes.len() as u64 > limit;
        Ok((bytes, exceeded))
    }
}

fn read(path: &Path, name: &str, ctx: &mut Context, budget: &mut ReadBudget) -> Option<String> {
    let limit = budget.per_file.min(budget.remaining as u64);
    let result = (|| -> std::io::Result<Vec<u8>> {
        if !fs::metadata(path)?.is_file() {
            return Err(std::io::Error::other("Not a regular file"));
        }
        let f = fs::File::open(path)?;
        let meta = f.metadata()?;
        if !meta.is_file() {
            return Err(std::io::Error::other("Not a regular file"));
        }
        if meta.len() > limit {
            ctx.incomplete = true;
            return Ok(Vec::new());
        }
        let (bytes, exceeded) = budget.read(f)?;
        if exceeded {
            ctx.incomplete = true;
            return Ok(Vec::new());
        }
        Ok(bytes)
    })();
    let bytes = match result {
        Ok(v) => v,
        Err(e) => {
            ctx.error = true;
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
            ctx.sources
                .push(json!({"path":name,"bytes":e.into_bytes()}));
            ctx.diag("BASIC-02", "Invalid UTF-8", name, 0, 0);
            return None;
        }
    };
    ctx.sources.push(json!({"path":name,"text":source}));
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
    let mut budget = ReadBudget::default();
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
    if manifest["schemaVersion"] != "loj/v1alpha1" {
        ctx.error = true;
        ctx.diag(
            "BASIC-01",
            "Unsupported schemaVersion",
            "law.yaml",
            0,
            source.len(),
        );
        ctx.diagnostics.last_mut().unwrap()["code"] = json!("loj/v1alpha1/unsupported-version");
        return None;
    }
    let schema: Value = serde_json::from_str(MANIFEST_SCHEMA).expect("embedded schema");
    let validator = jsonschema::options()
        .should_validate_formats(true)
        .build(&schema)
        .expect("embedded schema");
    let errors: Vec<_> = validator
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
            ctx.error = true;
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
            continue;
        }
        if fs::metadata(&physical).is_ok_and(|m| !m.is_file()) {
            ctx.diag("MAN-02", "Not a regular file", "law.yaml", 0, source.len());
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
                    continue;
                }
            }
            Err(e) => {
                ctx.error = true;
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
        let dir = std::env::temp_dir().join(format!("loj-budget-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for bytes in [b"\xffabc".as_slice(), b"\xef\xbb\xbfa", b"a\0bc", b"a\rbc"] {
            let path = dir.join("input.md");
            fs::write(&path, bytes).unwrap();
            let mut budget = ReadBudget {
                remaining: 7,
                per_file: 100,
            };
            let mut ctx = Context::default();
            assert!(read(&path, "first.md", &mut ctx, &mut budget).is_none());
            assert_eq!(budget.remaining, 3);
            assert_eq!(ctx.sources.len(), 1);
            assert!(!ctx.incomplete);
            assert!(read(&path, "second.md", &mut ctx, &mut budget).is_none());
            assert!(ctx.incomplete);
            assert_eq!(
                ctx.sources.len(),
                1,
                "over-budget bytes must not be retained"
            );
            assert_eq!(budget.remaining, 3, "metadata rejects before reading");
        }
    }

    #[test]
    fn streaming_read_is_bounded_even_when_length_is_unknown_or_changes() {
        for (remaining, per_file, expected_read) in [(7, 100, 8), (100, 7, 8)] {
            let mut budget = ReadBudget {
                remaining,
                per_file,
            };
            let (bytes, exceeded) = budget.read(std::io::repeat(b'x')).unwrap();
            assert!(exceeded);
            assert_eq!(bytes.len(), expected_read);
            assert_eq!(budget.remaining, remaining.saturating_sub(expected_read));
        }
        let mut budget = ReadBudget {
            remaining: 4,
            per_file: 4,
        };
        assert!(!budget.read(b"abcd".as_slice()).unwrap().1);
        assert_eq!(budget.remaining, 0);
    }

    #[test]
    fn mixed_valid_and_invalid_sources_share_exact_budget() {
        let dir = std::env::temp_dir().join(format!("loj-mixed-budget-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("input.md");
        let mut ctx = Context::default();
        let mut budget = ReadBudget {
            remaining: 8,
            per_file: 4,
        };
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
        assert_eq!(budget.remaining, 0);
        assert_eq!(ctx.sources.len(), 2);
        fs::write(&path, b"x").unwrap();
        assert!(read(&path, "overflow.md", &mut ctx, &mut budget).is_none());
        assert!(ctx.incomplete);
        assert_eq!(ctx.sources.len(), 2);
    }

    #[test]
    fn partial_io_failure_still_consumes_budget() {
        let mut budget = ReadBudget {
            remaining: 8,
            per_file: 8,
        };
        struct Fails;
        impl Read for Fails {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("simulated failure"))
            }
        }
        assert!(budget.read(b"abcd".as_slice().chain(Fails)).is_err());
        assert_eq!(budget.remaining, 4);
    }
}
