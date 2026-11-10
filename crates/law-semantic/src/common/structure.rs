//! Structural lines of a document (headings and reserved markers) and the
//! heading rules that loj/v1alpha1 and loj/v1alpha2 share.
use super::markdown::{blocks, line_end, line_start, reserved, BlockKind};
use regex::Regex;
use std::{ops::Range, sync::LazyLock};

/// A plain ATX heading line; group 1 is the label.
pub(crate) static ATX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#{1,6} ([^\r\n]+)$").expect("constant regex"));

/// Rank of an article heading; classification headings have ranks 1 to 5.
pub(crate) const ARTICLE_RANK: usize = 6;

/// One structural line of a document: a heading or a reserved marker.
pub(crate) struct Token {
    pub(crate) heading: bool,
    /// For a heading: CommonMark found nothing but plain text in it.
    pub(crate) plain: bool,
    pub(crate) span: Range<usize>,
}

/// The structural lines of `source`, in order, each widened to whole lines.
pub(crate) fn tokens(source: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    for (kind, span) in blocks(source) {
        let start = line_start(source, span.start);
        let end = line_end(source, span.end.saturating_sub(1));
        match kind {
            BlockKind::Heading(plain) => tokens.push(Token {
                heading: true,
                plain,
                span: start..end,
            }),
            BlockKind::Html if reserved(&source[start..end]) => tokens.push(Token {
                heading: false,
                plain: false,
                span: start..end,
            }),
            _ => {}
        }
    }
    tokens
}

/// HEAD-01: what may follow the number of a heading. An article takes an
/// optional caption in full-width parentheses; a classification takes one
/// space and a title.
pub(crate) fn tail_ok(rank: usize, tail: &str) -> bool {
    if rank == ARTICLE_RANK {
        tail.is_empty()
            || (tail.starts_with('（')
                && tail.ends_with('）')
                && tail.len() > 6
                && !tail[3..tail.len() - 3].ends_with(char::is_whitespace)
                && !tail[3..tail.len() - 3].contains(['（', '）', '(', ')']))
    } else {
        tail.strip_prefix([' ', '　'])
            .is_some_and(|s| !s.is_empty() && !s.starts_with([' ', '　']))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::markdown::bare;

    #[test]
    fn heading_tails() {
        for ok in ["", "（目的）", "（目 的）"] {
            assert!(tail_ok(ARTICLE_RANK, ok), "{ok:?}");
        }
        for bad in [
            "（）",
            "（目的） ",
            "（目的 ）",
            "（目（的））",
            "（目(的）",
            "(目的)",
            " 目的",
            "　（目的）",
        ] {
            assert!(!tail_ok(ARTICLE_RANK, bad), "{bad:?}");
        }
        for ok in ["　総則", " 総則", "　総則　通則"] {
            assert!(tail_ok(2, ok), "{ok:?}");
        }
        for bad in ["", "総則", "　", "　　総則", " 　総則", "（総則）"] {
            assert!(!tail_ok(2, bad), "{bad:?}");
        }
    }

    #[test]
    fn tokens_are_headings_and_reserved_markers_as_whole_lines() {
        let source = "# 第一条\r\n\r\n<!-- loj:paragraph number=1 -->\r\n本文 *強調*\r\n\
                      <!-- loj:end-paragraph -->\r\n\r\n<!-- 注記 -->\r\n## 第二条 *x*\r\n";
        let found: Vec<(bool, bool, &str)> = tokens(source)
            .iter()
            .map(|t| (t.heading, t.plain, bare(&source[t.span.clone()])))
            .collect();
        assert_eq!(
            found,
            [
                (true, true, "# 第一条"),
                (false, false, "<!-- loj:paragraph number=1 -->"),
                (false, false, "<!-- loj:end-paragraph -->"),
                (true, false, "## 第二条 *x*"),
            ]
        );
    }
}
