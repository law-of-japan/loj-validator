//! CORE-03: YAML 1.2 restricted to the JSON-compatible data model.
//! Duplicate keys, tags, anchors, aliases, merge keys and multiple documents are rejected.
use serde_json::{Map, Value};
use yaml_rust2::{
    parser::{Event, Parser},
    scanner::{Scanner, TScalarStyle, TokenType},
};

/// Parse one YAML document into JSON values.
pub fn parse(source: &str) -> Result<Value, String> {
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
        if depth > 64 {
            return Err("YAML nesting exceeds 64".into());
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
                        return Err(format!("Duplicate or merge key: {key}"));
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_data_model_only() {
        assert_eq!(parse("a: 1\nb: \"x\"\n").unwrap()["a"], 1);
        for bad in [
            "a: 1\na: 2\n",
            "a: &x 1\nb: *x\n",
            "a: !!str 1\n",
            "a: 2020-01-01\n",
            "a:\n",
            "--- 1\n--- 2\n",
            "<<: {a: 1}\n",
            "a: yes_but_True\nb: True\n",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        // Text is preserved exactly (no normalization).
        assert_eq!(parse("a: \"\u{fa51}\"\n").unwrap()["a"], "\u{fa51}");
    }
}
