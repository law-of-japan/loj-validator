//! Strict NUM-01 normalization (loj/v1alpha1 and loj/v1alpha2), independent of legacy numbering.
use std::sync::OnceLock;
fn japanese(n: u32) -> String {
    let digits = ['〇', '一', '二', '三', '四', '五', '六', '七', '八', '九'];
    let mut s = String::new();
    for (unit, c) in [(1000, '千'), (100, '百'), (10, '十')] {
        let d = n / unit % 10;
        if d > 0 {
            if d > 1 {
                s.push(digits[d as usize]);
            }
            s.push(c);
        }
    }
    if !n.is_multiple_of(10) {
        s.push(digits[(n % 10) as usize]);
    }
    s
}
/// Which digit forms one part of a number may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Digits {
    /// Kanji numerals, ASCII digits or full-width digits: heading labels
    /// and the display token of an item.
    Any,
    /// ASCII or full-width digits: the display token of a paragraph.
    Arabic,
    /// ASCII digits only: marker attributes and the display token of a subitem.
    Ascii,
}

fn component(s: &str, digits: Digits) -> Option<String> {
    if s.is_empty() {
        return None;
    }
    let ascii = s.chars().all(|c| c.is_ascii_digit());
    let wide = s.chars().all(|c| ('０'..='９').contains(&c));
    let value = if ascii || (wide && digits != Digits::Ascii) {
        let t: String = s
            .chars()
            .map(|c| {
                if wide {
                    char::from_digit(u32::from(c) - u32::from('０'), 10).unwrap_or(c)
                } else {
                    c
                }
            })
            .collect();
        if t.starts_with('0') || t.len() > 4 {
            return None;
        }
        t.parse::<u32>().ok()?
    } else if digits == Digits::Any {
        static NUMBERS: OnceLock<std::collections::HashMap<String, u32>> = OnceLock::new();
        *NUMBERS
            .get_or_init(|| (1..=9999).map(|n| (japanese(n), n)).collect())
            .get(s)?
    } else {
        return None;
    };
    (1..=9999).contains(&value).then(|| value.to_string())
}
/// `separators`: `-` for attributes; for labels and display tokens, `の` alone
/// (loj/v1alpha1) or `の`/`ノ` (loj/v1alpha2, NUM-01).
pub(crate) fn number(s: &str, separators: &[char], digits: Digits) -> Option<String> {
    s.split(separators)
        .map(|s| component(s, digits))
        .collect::<Option<Vec<_>>>()
        .map(|v| v.join("-"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_numbers_and_width_boundaries() {
        for n in 1..=9999 {
            assert_eq!(component(&japanese(n), Digits::Any), Some(n.to_string()));
        }
        for bad in [
            "0", "01", "０１", "１2", "一千", "一百", "一十", "一二", "〇", "零", "10000",
        ] {
            assert_eq!(component(bad, Digits::Any), None, "{bad}");
        }
        assert_eq!(
            number("十二の二の３", &['の', 'ノ'], Digits::Any),
            Some("12-2-3".into())
        );
        assert_eq!(
            number("十二ノ二の三", &['の', 'ノ'], Digits::Any),
            Some("12-2-3".into())
        );
        // loj/v1alpha1 passes `の` alone, so `ノ` is not a separator there.
        assert_eq!(
            number("十二の二の３", &['の'], Digits::Any),
            Some("12-2-3".into())
        );
        assert_eq!(number("十二ノ二", &['の'], Digits::Any), None);
        assert_eq!(number("千十", &['の'], Digits::Any), Some("1010".into()));
        assert_eq!(number("1-01", &['-'], Digits::Ascii), None);
        assert_eq!(number("１", &['-'], Digits::Ascii), None);
        // Full-width digits: allowed for `Any` and `Arabic`, never for `Ascii`.
        assert_eq!(number("１２", &['-'], Digits::Arabic), Some("12".into()));
        assert_eq!(number("12", &['-'], Digits::Arabic), Some("12".into()));
        assert_eq!(number("12", &['-'], Digits::Ascii), Some("12".into()));
        // Kanji numerals: only for `Any`.
        assert_eq!(number("十二", &['-'], Digits::Arabic), None);
        assert_eq!(number("十二", &['-'], Digits::Ascii), None);
    }
}
