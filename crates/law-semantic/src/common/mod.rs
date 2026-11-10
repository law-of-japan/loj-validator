//! Shared infrastructure for the next specification (`loj/v1alpha2` and the
//! `loj-history`, `loj-glyphs`, `loj-registry`, `loj-rebuild` kinds):
//! strict byte and YAML loading (CORE-03), and the check report
//! (`schemas/common/v1alpha1/report.schema.json`).
pub(crate) mod budget;
pub mod findings;
pub(crate) mod markdown;
pub(crate) mod number;
pub mod report;
pub mod time;
pub mod yaml;

pub use report::{Check, Diagnostic, Report, ResultValue};

/// loj-spec commit of the vendored next specification.
pub const SPEC_COMMIT: &str = "4ac8f2b47c98e76fa5ccda35d7a3ee60f0816ad4";
/// Rule namespace of the common requirements.
pub const COMMON: &str = "common/v1alpha1";
/// Per-file read limit; larger inputs are reported as incomplete.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// Why strict text decoding failed (BASIC-02 / CORE-03 bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BytesError {
    Empty,
    Utf8,
    Bom,
    Nul,
    BareCr,
}

impl std::fmt::Display for BytesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            BytesError::Empty => "empty file",
            BytesError::Utf8 => "invalid UTF-8",
            BytesError::Bom => "byte order mark",
            BytesError::Nul => "NUL character",
            BytesError::BareCr => "bare CR line ending",
        })
    }
}

/// Decode strict UTF-8 without BOM, NUL or bare CR. The text is never normalized.
pub fn strict_text(bytes: Vec<u8>) -> Result<String, BytesError> {
    if bytes.is_empty() {
        return Err(BytesError::Empty);
    }
    let text = String::from_utf8(bytes).map_err(|_| BytesError::Utf8)?;
    if text.starts_with('\u{feff}') {
        return Err(BytesError::Bom);
    }
    if text.contains('\0') {
        return Err(BytesError::Nul);
    }
    let b = text.as_bytes();
    if b.iter()
        .enumerate()
        .any(|(i, c)| *c == b'\r' && b.get(i + 1) != Some(&b'\n'))
    {
        return Err(BytesError::BareCr);
    }
    Ok(text)
}

/// Outcome of reading one YAML file of a kind.
pub enum Loaded {
    /// Parsed document and its source text.
    Ok(serde_json::Value, String),
    /// The file is missing or unreadable (incomplete).
    Unreadable(String),
    /// Bytes or YAML data model violation (the kind's format rule).
    Invalid(String),
}

/// Read a YAML file under CORE-03.
pub fn load_yaml(path: &std::path::Path) -> Loaded {
    let bytes = match read_limited(path) {
        Ok(b) => b,
        Err(e) => return Loaded::Unreadable(e),
    };
    let text = match strict_text(bytes) {
        Ok(t) => t,
        Err(e) => return Loaded::Invalid(e.to_string()),
    };
    match yaml::parse(&text) {
        Ok(v) if v.is_object() => Loaded::Ok(v, text),
        Ok(_) => Loaded::Invalid("top level must be a mapping".into()),
        Err(e) => Loaded::Invalid(e),
    }
}

/// Read a regular file, refusing anything over [`MAX_FILE_BYTES`].
pub fn read_limited(path: &std::path::Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a regular file".into());
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err("file exceeds the 16 MiB implementation limit".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("file exceeds the 16 MiB implementation limit".into());
    }
    Ok(bytes)
}

/// Compile one of the vendored JSON Schemas with `format` assertions enabled.
/// `resources` maps `$id` URIs of referenced schemas to their text.
pub fn validator(schema: &str, resources: &[&str]) -> jsonschema::Validator {
    struct Embedded(std::collections::HashMap<String, serde_json::Value>);
    impl jsonschema::Retrieve for Embedded {
        fn retrieve(
            &self,
            uri: &jsonschema::Uri<String>,
        ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
            self.0
                .get(uri.as_str())
                .cloned()
                .ok_or_else(|| format!("schema not vendored: {}", uri.as_str()).into())
        }
    }
    let schema: serde_json::Value = serde_json::from_str(schema).expect("embedded schema");
    let mut map = std::collections::HashMap::new();
    for r in resources {
        let v: serde_json::Value = serde_json::from_str(r).expect("embedded schema");
        let id = v["$id"].as_str().expect("schema $id").to_owned();
        map.insert(id, v);
    }
    jsonschema::options()
        .should_validate_formats(true)
        .with_retriever(Embedded(map))
        .build(&schema)
        .expect("embedded schema")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_text_rejects_each_byte_violation() {
        assert_eq!(strict_text(vec![]), Err(BytesError::Empty));
        assert_eq!(strict_text(vec![0xff]), Err(BytesError::Utf8));
        assert_eq!(strict_text(b"\xef\xbb\xbfa".to_vec()), Err(BytesError::Bom));
        assert_eq!(strict_text(b"a\0".to_vec()), Err(BytesError::Nul));
        assert_eq!(strict_text(b"a\rb".to_vec()), Err(BytesError::BareCr));
        assert_eq!(strict_text(b"a\r\n".to_vec()).unwrap(), "a\r\n");
        // CJK compatibility ideographs survive untouched (CHAR-01).
        let s = "\u{fa51}\u{fa19}";
        assert_eq!(strict_text(s.as_bytes().to_vec()).unwrap(), s);
    }
}
