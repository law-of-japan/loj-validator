//! CommonMark regions and byte-preserving line boundaries.
use pulldown_cmark::{Event, Parser, Tag};
use std::ops::Range;
#[derive(Clone, Copy)]
pub(super) enum BlockKind {
    Heading(bool),
    Html,
    Other,
}
pub(super) fn blocks(source: &str) -> Vec<(BlockKind, Range<usize>)> {
    let mut result = Vec::new();
    let mut depth = 0;
    let mut current = None;
    for (event, span) in Parser::new(source).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                if depth == 0 {
                    current = Some((
                        match tag {
                            Tag::Heading { .. } => BlockKind::Heading(true),
                            Tag::HtmlBlock => BlockKind::Html,
                            _ => BlockKind::Other,
                        },
                        span.clone(),
                    ));
                } else if let Some((BlockKind::Heading(plain), _)) = current.as_mut() {
                    *plain = false;
                }
                depth += 1;
            }
            Event::End(_) => {
                depth -= 1;
                if depth == 0 {
                    if let Some((kind, mut r)) = current.take() {
                        r.end = span.end;
                        result.push((kind, r));
                    }
                }
            }
            _ if depth == 0 => result.push((BlockKind::Other, span)),
            Event::Text(_) => {}
            _ => {
                if let Some((BlockKind::Heading(plain), _)) = current.as_mut() {
                    *plain = false;
                }
            }
        }
    }
    result
}
pub(super) fn line_start(s: &str, at: usize) -> usize {
    s[..at].rfind('\n').map_or(0, |i| i + 1)
}
pub(super) fn line_end(s: &str, at: usize) -> usize {
    s.as_bytes()[at..]
        .iter()
        .position(|b| *b == b'\n')
        .map_or(s.len(), |i| at + i + 1)
}
pub(super) fn bare(s: &str) -> &str {
    s.strip_suffix('\n')
        .unwrap_or(s)
        .strip_suffix('\r')
        .unwrap_or(s.strip_suffix('\n').unwrap_or(s))
}
pub(super) fn reserved(s: &str) -> bool {
    s.trim_start_matches(' ')
        .strip_prefix("<!--")
        .is_some_and(|v| {
            v.trim_start_matches(|c: char| c.is_ascii_whitespace())
                .get(..4)
                .is_some_and(|p| p.eq_ignore_ascii_case("loj:"))
        })
}

pub(super) fn plain_heading(raw: &str, label: &str) -> bool {
    let mut text = String::new();
    for event in Parser::new(raw) {
        match event {
            Event::Start(Tag::Heading { .. }) | Event::End(pulldown_cmark::TagEnd::Heading(_)) => {}
            Event::Text(t) => text.push_str(&t),
            _ => return false,
        }
    }
    text == label
}
