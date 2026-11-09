//! Strict NUM-01 normalization (loj/v1alpha2), independent of legacy numbering.
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
fn component(s: &str, mode: u8) -> Option<String> {
    if s.is_empty() {
        return None;
    }
    let ascii = s.chars().all(|c| c.is_ascii_digit());
    let wide = s.chars().all(|c| ('０'..='９').contains(&c));
    let value = if ascii || (wide && mode != 2) {
        let t: String = s
            .chars()
            .map(|c| {
                if wide {
                    char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap()
                } else {
                    c
                }
            })
            .collect();
        if t.starts_with('0') || t.len() > 4 {
            return None;
        }
        t.parse::<u32>().ok()?
    } else if mode == 0 {
        static NUMBERS: OnceLock<std::collections::HashMap<String, u32>> = OnceLock::new();
        *NUMBERS
            .get_or_init(|| (1..=9999).map(|n| (japanese(n), n)).collect())
            .get(s)?
    } else {
        return None;
    };
    (1..=9999).contains(&value).then(|| value.to_string())
}
/// `separators`: `の`/`ノ` for labels and display tokens (NUM-01), `-` for attributes.
pub(super) fn number(s: &str, separators: &[char], mode: u8) -> Option<String> {
    s.split(separators)
        .map(|s| component(s, mode))
        .collect::<Option<Vec<_>>>()
        .map(|v| v.join("-"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_numbers_and_width_boundaries() {
        for n in 1..=9999 {
            assert_eq!(component(&japanese(n), 0), Some(n.to_string()));
        }
        for bad in [
            "0", "01", "０１", "１2", "一千", "一百", "一十", "一二", "〇", "零", "10000",
        ] {
            assert_eq!(component(bad, 0), None, "{bad}");
        }
        assert_eq!(
            number("十二の二の３", &['の', 'ノ'], 0),
            Some("12-2-3".into())
        );
        assert_eq!(
            number("十二ノ二の三", &['の', 'ノ'], 0),
            Some("12-2-3".into())
        );
        assert_eq!(number("千十", &['の'], 0), Some("1010".into()));
        assert_eq!(number("1-01", &['-'], 2), None);
        assert_eq!(number("１", &['-'], 2), None);
    }
}
