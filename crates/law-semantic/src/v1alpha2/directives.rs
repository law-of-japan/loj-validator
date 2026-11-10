//! DIR-01 lexical grammar (grammar/v1alpha2/directives.ebnf), applied only to
//! eligible CommonMark regions.
use super::kind::Kind;
use crate::common::number::{number, Digits};

/// A reserved marker line.
pub(super) enum Marker {
    /// `end-…`: closes the innermost container of this kind.
    Close(Kind),
    Preamble,
    EnactStatement,
    Open(Container),
}

/// An opening marker of a container, with the attributes its kind requires.
pub(super) enum Container {
    Paragraph {
        number: String,
        implicit: bool,
    },
    Item {
        number: String,
    },
    Subitem {
        number: String,
        level: usize,
    },
    Table {
        number: String,
    },
    Row {
        key: String,
    },
    Cell {
        rowspan: Option<u64>,
        colspan: Option<u64>,
    },
}

impl Container {
    pub(super) fn kind(&self) -> Kind {
        match self {
            Container::Paragraph { .. } => Kind::Paragraph,
            Container::Item { .. } => Kind::Item,
            Container::Subitem { .. } => Kind::Subitem,
            Container::Table { .. } => Kind::Table,
            Container::Row { .. } => Kind::Row,
            Container::Cell { .. } => Kind::Cell,
        }
    }
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
    if let Some(k) = inner.strip_prefix("end-") {
        return Ok(Marker::Close(Kind::from_marker(k).ok_or_else(invalid)?));
    }
    if inner.contains(['\t', '\r', '\n']) || inner.starts_with(' ') || inner.ends_with(' ') {
        return Err(invalid());
    }
    let mut words = inner.split(' ').filter(|s| !s.is_empty());
    let kind = Kind::from_marker(words.next().ok_or_else(invalid)?).ok_or_else(invalid)?;
    let allowed: &[&str] = match kind {
        Kind::Paragraph => &["number", "numbering"],
        Kind::Item | Kind::Table => &["number"],
        Kind::Subitem => &["number", "level"],
        Kind::Row => &["key"],
        Kind::Cell => &["rowspan", "colspan"],
        _ => &[],
    };
    let (mut num, mut level, mut numbering, mut key) = (None, None, None, None);
    let (mut rowspan, mut colspan) = (None, None);
    let mut error: Option<Invalid> = None;
    let mut seen = std::collections::HashSet::new();
    for word in words {
        let (k, v) = word.split_once('=').ok_or_else(invalid)?;
        if !seen.insert(k) || !allowed.contains(&k) {
            return Err(invalid());
        }
        match k {
            "number" => num = Some(number(v, &['-'], Digits::Ascii).ok_or_else(invalid)?),
            "level" => {
                let n = v.parse::<usize>().ok().ok_or_else(invalid)?;
                if n == 0 || n > 10 || n.to_string() != v {
                    return Err(invalid());
                }
                level = Some(n);
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
                key = Some(v.to_owned());
            }
            "rowspan" | "colspan" => match span(v) {
                Ok(n) if k == "rowspan" => rowspan = Some(n),
                Ok(n) => colspan = Some(n),
                Err(e) => error = Some(e),
            },
            _ => unreachable!(),
        }
    }
    if let Some(e) = error {
        return Err(e);
    }
    // A required attribute that is missing is a lexical error (DIR-01).
    let number = num.ok_or_else(invalid);
    Ok(match kind {
        Kind::Preamble => Marker::Preamble,
        Kind::EnactStatement => Marker::EnactStatement,
        Kind::Paragraph => Marker::Open(Container::Paragraph {
            number: number?,
            implicit: numbering.unwrap_or(false),
        }),
        Kind::Item => Marker::Open(Container::Item { number: number? }),
        Kind::Table => Marker::Open(Container::Table { number: number? }),
        Kind::Subitem => Marker::Open(Container::Subitem {
            number: number?,
            level: level.ok_or_else(invalid)?,
        }),
        Kind::Row => Marker::Open(Container::Row {
            key: key.ok_or(Invalid {
                also: Some("TABLE-02"),
            })?,
        }),
        Kind::Cell => Marker::Open(Container::Cell { rowspan, colspan }),
        // `Kind::from_marker` returns no other kind.
        _ => return Err(invalid()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grammar() {
        assert!(matches!(
            marker("<!-- loj:paragraph numbering=implicit number=2 -->"),
            Ok(Marker::Open(Container::Paragraph { implicit: true, number })) if number == "2"
        ));
        assert!(matches!(
            marker("<!-- loj:paragraph number=3 -->"),
            Ok(Marker::Open(Container::Paragraph {
                implicit: false,
                ..
            }))
        ));
        assert!(matches!(
            marker("<!-- loj:subitem number=1 level=2 -->"),
            Ok(Marker::Open(Container::Subitem { level: 2, .. }))
        ));
        assert!(matches!(
            marker("<!-- loj:row key=k3v9 -->"),
            Ok(Marker::Open(Container::Row { key })) if key == "k3v9"
        ));
        assert!(matches!(
            marker("<!-- loj:cell colspan=2 rowspan=10 -->"),
            Ok(Marker::Open(Container::Cell {
                rowspan: Some(10),
                colspan: Some(2)
            }))
        ));
        assert!(matches!(
            marker("<!-- loj:preamble -->"),
            Ok(Marker::Preamble)
        ));
        // A required attribute that is missing is refused, never defaulted.
        for bad in [
            "<!-- loj:item -->",
            "<!-- loj:table -->",
            "<!-- loj:subitem number=1 -->",
            "<!-- loj:subitem level=1 -->",
        ] {
            assert!(marker(bad).is_err_and(|e| e.also.is_none()), "{bad}");
        }
        assert!(marker("<!-- loj:row -->").is_err_and(|e| e.also == Some("TABLE-02")));
        assert!(marker("<!-- loj:subitem number=1 level=2 -->").is_ok());
        assert!(marker("<!-- loj:cell -->").is_ok());
        assert!(marker("<!-- loj:cell colspan=2 rowspan=10 -->").is_ok());
        assert!(marker("<!-- loj:enact-statement -->").is_ok());
        assert!(matches!(
            marker("<!-- loj:end-preamble -->"),
            Ok(Marker::Close(Kind::Preamble))
        ));
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
