//! DIR-01 lexical grammar, applied only to eligible CommonMark regions.
use crate::common::number::{number, Digits};
pub(super) struct Marker {
    pub(super) kind: &'static str,
    pub(super) close: bool,
    pub(super) number: String,
    pub(super) level: usize,
}
pub(super) fn marker(s: &str) -> Option<Marker> {
    let inner = s.strip_prefix("<!-- loj:")?.strip_suffix(" -->")?;
    if let Some(k) = inner.strip_prefix("end-") {
        return Some(Marker {
            kind: match k {
                "paragraph" => "Paragraph",
                "item" => "Item",
                "subitem" => "Subitem",
                _ => return None,
            },
            close: true,
            number: String::new(),
            level: 0,
        });
    }
    if inner.contains(['\t', '\r', '\n']) || inner.starts_with(' ') || inner.ends_with(' ') {
        return None;
    }
    let mut words = inner.split(' ').filter(|s| !s.is_empty());
    let kind = match words.next()? {
        "paragraph" => "Paragraph",
        "item" => "Item",
        "subitem" => "Subitem",
        _ => return None,
    };
    let mut num = None;
    let mut level = None;
    for word in words {
        let (k, v) = word.split_once('=')?;
        match k {
            "number" if num.is_none() => num = Some(number(v, &['-'], Digits::Ascii)?),
            "level" if level.is_none() => {
                let n = v.parse::<usize>().ok()?;
                if n == 0 || n > 10 || n.to_string() != v {
                    return None;
                }
                level = Some(n);
            }
            _ => return None,
        }
    }
    if (kind == "Subitem") != level.is_some() {
        return None;
    }
    Some(Marker {
        kind,
        close: false,
        number: num?,
        level: level.unwrap_or(0),
    })
}
