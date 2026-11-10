//! DIR-01 lexical grammar (grammar/v1alpha2/directives.ebnf), applied only to
//! eligible CommonMark regions.
use crate::common::number::number;

pub(super) struct Marker {
    pub(super) kind: &'static str,
    pub(super) close: bool,
    pub(super) number: Option<String>,
    pub(super) level: usize,
    pub(super) implicit: bool,
    pub(super) key: Option<String>,
    pub(super) rowspan: Option<u64>,
    pub(super) colspan: Option<u64>,
}

fn kind(name: &str) -> Option<&'static str> {
    Some(match name {
        "paragraph" => "Paragraph",
        "item" => "Item",
        "subitem" => "Subitem",
        "table" => "Table",
        "row" => "Row",
        "cell" => "Cell",
        "preamble" => "Preamble",
        "enact-statement" => "EnactStatement",
        _ => return None,
    })
}

/// A lexical error: DIR-01 plus any rule that constrains the offending value.
pub(super) struct Invalid {
    pub(super) also: Option<&'static str>,
}

fn invalid() -> Invalid {
    Invalid { also: None }
}

fn span(v: &str) -> Result<u64, Invalid> {
    let n = v
        .parse::<u64>()
        .ok()
        .filter(|n| n.to_string() == v && *n >= 1)
        .ok_or_else(invalid)?;
    if n == 1 {
        // TABLE-03: the default 1 must not be written.
        return Err(Invalid {
            also: Some("TABLE-03"),
        });
    }
    Ok(n)
}

pub(super) fn marker(s: &str) -> Result<Marker, Invalid> {
    let inner = s
        .strip_prefix("<!-- loj:")
        .and_then(|s| s.strip_suffix(" -->"))
        .ok_or_else(invalid)?;
    let mut m = Marker {
        kind: "",
        close: false,
        number: None,
        level: 0,
        implicit: false,
        key: None,
        rowspan: None,
        colspan: None,
    };
    if let Some(k) = inner.strip_prefix("end-") {
        m.kind = kind(k).ok_or_else(invalid)?;
        m.close = true;
        return Ok(m);
    }
    if inner.contains(['\t', '\r', '\n']) || inner.starts_with(' ') || inner.ends_with(' ') {
        return Err(invalid());
    }
    let mut words = inner.split(' ').filter(|s| !s.is_empty());
    m.kind = kind(words.next().ok_or_else(invalid)?).ok_or_else(invalid)?;
    let mut numbering = None;
    let mut error: Option<Invalid> = None;
    let mut seen = std::collections::HashSet::new();
    for word in words {
        let (k, v) = word.split_once('=').ok_or_else(invalid)?;
        if !seen.insert(k) {
            return Err(invalid());
        }
        let allowed: &[&str] = match m.kind {
            "Paragraph" => &["number", "numbering"],
            "Item" | "Table" => &["number"],
            "Subitem" => &["number", "level"],
            "Row" => &["key"],
            "Cell" => &["rowspan", "colspan"],
            _ => &[],
        };
        if !allowed.contains(&k) {
            return Err(invalid());
        }
        match k {
            "number" => m.number = Some(number(v, &['-'], 2).ok_or_else(invalid)?),
            "level" => {
                let n = v.parse::<usize>().ok().ok_or_else(invalid)?;
                if n == 0 || n > 10 || n.to_string() != v {
                    return Err(invalid());
                }
                m.level = n;
            }
            "numbering" => {
                numbering = Some(match v {
                    "explicit" => false,
                    "implicit" => true,
                    _ => return Err(invalid()),
                })
            }
            "key" => {
                // TABLE-02: lowercase Crockford Base32, four characters.
                if v.len() != 4
                    || !v
                        .bytes()
                        .all(|b| b.is_ascii_digit() || b"abcdefghjkmnpqrstvwxyz".contains(&b))
                {
                    return Err(Invalid {
                        also: Some("TABLE-02"),
                    });
                }
                m.key = Some(v.to_owned());
            }
            "rowspan" | "colspan" => match span(v) {
                Ok(n) if k == "rowspan" => m.rowspan = Some(n),
                Ok(n) => m.colspan = Some(n),
                Err(e) => error = Some(e),
            },
            _ => unreachable!(),
        }
    }
    if let Some(e) = error {
        return Err(e);
    }
    m.implicit = numbering.unwrap_or(false);
    let required: &[&str] = match m.kind {
        "Paragraph" | "Item" | "Table" => &["number"],
        "Subitem" => &["number", "level"],
        "Row" => &["key"],
        _ => &[],
    };
    if required.iter().any(|r| !seen.contains(r)) {
        return Err(Invalid {
            also: (m.kind == "Row").then_some("TABLE-02"),
        });
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grammar() {
        assert!(
            marker("<!-- loj:paragraph numbering=implicit number=2 -->").is_ok_and(|m| m.implicit)
        );
        assert!(marker("<!-- loj:subitem number=1 level=2 -->").is_ok());
        assert!(marker("<!-- loj:cell -->").is_ok());
        assert!(marker("<!-- loj:cell colspan=2 rowspan=10 -->").is_ok());
        assert!(marker("<!-- loj:enact-statement -->").is_ok());
        assert!(marker("<!-- loj:end-preamble -->").is_ok_and(|m| m.close));
        for bad in [
            "<!-- loj:paragraph -->",
            "<!-- loj:paragraph number=1 number=1 -->",
            "<!-- loj:paragraph number=01 -->",
            "<!-- loj:paragraph number=1 numbering=none -->",
            "<!-- loj:item number=1 numbering=explicit -->",
            "<!-- loj:preamble number=1 -->",
            "<!-- loj:end-paragraph number=1 -->",
            "<!-- loj:Paragraph number=1 -->",
            "<!-- loj:paragraph\tnumber=1 -->",
        ] {
            assert!(marker(bad).is_err(), "{bad}");
        }
        assert_eq!(
            marker("<!-- loj:row key=K3V9 -->").err().unwrap().also,
            Some("TABLE-02")
        );
        assert_eq!(
            marker("<!-- loj:row key=k3vi -->").err().unwrap().also,
            Some("TABLE-02")
        );
        assert_eq!(
            marker("<!-- loj:cell colspan=1 -->").err().unwrap().also,
            Some("TABLE-03")
        );
    }
}
