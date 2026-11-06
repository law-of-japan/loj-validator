//! CommonMark regions and byte-preserving line boundaries.
use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
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

/// MAN-03 adds explicit termination only at nonfinal appendix boundaries.
/// Use CommonMark events for block recognition, including nested containers.
pub(super) fn incomplete_split_blocks(source: &str) -> Vec<Range<usize>> {
    let mut result = Vec::new();
    // CommonMark HTML tag matching is ASCII case-insensitive, not Unicode folding.
    let type_one =
        regex::Regex::new(r"(?i-u)^<(script|pre|style|textarea)(?:[\t\r\n >]|$)").unwrap();
    let type_one_end = regex::Regex::new(r"(?i-u)</(script|pre|style|textarea)>").unwrap();
    let mut fence = None;
    let mut html = None;
    let mut quotes = 0usize;
    for (event, span) in Parser::new(source).into_offset_iter() {
        match event {
            Event::Start(Tag::BlockQuote(_)) => quotes += 1,
            Event::End(TagEnd::BlockQuote(_)) => quotes -= 1,
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_))) => {
                fence = Some((span.start, line_end(source, span.start)));
            }
            Event::Text(_) if fence.is_some() => {
                fence.as_mut().unwrap().1 = span.end;
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((start, body_end)) = fence.take() {
                    // The parser's closing range includes an explicit fence,
                    // but an implicit EOF/container end stops at the body end.
                    if span.end <= body_end {
                        result.push(line_start(source, start)..line_end(source, start));
                    }
                }
            }
            Event::Start(Tag::HtmlBlock) => html = Some((span.start, String::new())),
            Event::Html(text) if html.is_some() => html.as_mut().unwrap().1.push_str(&text),
            Event::End(TagEnd::HtmlBlock) => {
                if let Some((start, text)) = html.take() {
                    let text = text.trim_start();
                    let closed = if type_one.is_match(text) {
                        type_one_end.is_match(text)
                    } else if text.starts_with("<!--") {
                        text.contains("-->")
                    } else if text.starts_with("<?") {
                        text.contains("?>")
                    } else if text.starts_with("<![CDATA[") {
                        text.contains("]]>")
                    } else if text.starts_with("<!")
                        && text.as_bytes().get(2).is_some_and(u8::is_ascii_alphabetic)
                    {
                        text.contains('>')
                    } else {
                        // Types 6/7 stop before the terminating blank line.
                        // Container prefixes may precede that blank line.
                        let tail = &source[span.end..line_end(source, span.end)];
                        let mut prefix_quotes = 0;
                        !tail.is_empty()
                            && tail.bytes().all(|b| match b {
                                b' ' | b'\t' | b'\r' | b'\n' => true,
                                b'>' => {
                                    prefix_quotes += 1;
                                    prefix_quotes <= quotes
                                }
                                _ => false,
                            })
                    };
                    if !closed {
                        result.push(line_start(source, start)..line_end(source, start));
                    }
                }
            }
            _ => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_fences_require_parser_recognized_closers() {
        for source in [
            "```\nx\n```",
            "~~~\nx\n~~~~ \n",
            "  ```\nx\n ```\n",
            "> ```\n> x\n> ```\n",
            "- ```\n  x\n  ```\n",
            "```\r\nx\r\n```\r\n",
            "    ```\n    x\n",
            "paragraph `code`\n",
            "```\n```\n",
            "```\nx\n```\n\n```\ny\n```\n",
        ] {
            assert!(incomplete_split_blocks(source).is_empty(), "{source:?}");
        }
        for source in [
            "```",
            "```\n",
            "```\nx",
            "```\nx\n",
            "````\nx\n```\n",
            "```\nx\n~~~\n",
            "```\nx\n    ```\n",
            "```\nx\n``` trailing\n",
            "> ```\n> x\n\noutside\n",
            "- ```\n  x\n\noutside\n",
            "```\r\nx\r\n",
        ] {
            assert_eq!(
                incomplete_split_blocks(source),
                vec![0..line_end(source, 0)],
                "{source:?}"
            );
        }
    }

    #[test]
    fn split_html_requires_its_own_terminator() {
        for source in [
            "<SCRIPT>\nx\n</script>\n",
            "<!-- x -->\n",
            "<?x?>\n",
            "<!DOCTYPE html>\n",
            "<![CDATA[x]]>\n",
            "<div>\nx\n\n",
            "<custom>\nx\n\n",
            "> <div>\n> x\n>\n",
            "- <div>\n  x\n\n",
            "> <!--x-->\n",
            "```\n<div>\n```\n",
            "    <div>\n",
            "<div>\r\nx\r\n\r\n",
        ] {
            assert!(incomplete_split_blocks(source).is_empty(), "{source:?}");
        }
        for source in [
            "<script>\nx\n",
            "<script>\n</ſcript>\n",
            "<!-- x\n",
            "<?x\n",
            "<!DOCTYPE\n",
            "<![CDATA[x\n",
            "<div>\nx\n</div>\n",
            "<custom>\nx\n",
            "> <div>\n> x\n",
            "> <!--x\n\noutside\n",
            "- <div>\n  x\n",
            "<div>\r\nx\r\n",
        ] {
            assert_eq!(
                incomplete_split_blocks(source),
                vec![0..line_end(source, 0)],
                "{source:?}"
            );
        }
    }
}
