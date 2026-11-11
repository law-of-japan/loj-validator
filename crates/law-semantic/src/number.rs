//! Law-number normalization only; not a natural-language reference parser.
pub fn normalize(input: &str) -> Result<Vec<u64>, String> {
    input.split(['の', 'ノ']).map(component).collect()
}

fn digit(c: char) -> Option<u64> {
    match c {
        '〇' | '零' => Some(0),
        '一' => Some(1),
        '二' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    }
}

fn small(input: &str) -> Result<u64, String> {
    if input.is_empty() {
        return Err("empty numeral group".into());
    }
    let mut total = 0;
    let mut pending = None;
    let mut last_unit = 10_000;
    for c in input.chars() {
        if let Some(d) = digit(c) {
            if d == 0 || pending.is_some() {
                return Err("zero or adjacent digits in unit notation".into());
            }
            pending = Some(d);
        } else {
            let unit = match c {
                '十' => 10,
                '百' => 100,
                '千' => 1000,
                _ => return Err(format!("unsupported numeral: {c}")),
            };
            if unit >= last_unit {
                return Err("units must strictly descend".into());
            }
            total += pending.take().unwrap_or(1) * unit;
            last_unit = unit;
        }
    }
    Ok(total + pending.unwrap_or(0))
}

fn component(input: &str) -> Result<u64, String> {
    if input.is_empty() {
        return Err("empty number component".into());
    }
    let chars: Vec<char> = input.chars().collect();
    let ascii: String = chars
        .iter()
        .map(|&c| {
            if ('０'..='９').contains(&c) {
                char::from_digit(u32::from(c) - u32::from('０'), 10).unwrap_or(c)
            } else {
                c
            }
        })
        .collect();
    let value = if ascii.chars().all(|c| c.is_ascii_digit()) {
        ascii
            .parse::<u64>()
            .map_err(|_| "number overflow".to_string())?
    } else if let Some(digits) = chars
        .iter()
        .map(|&c| digit(c))
        .collect::<Option<Vec<u64>>>()
    {
        digits.iter().try_fold(0u64, |n, &d| {
            n.checked_mul(10)
                .and_then(|x| x.checked_add(d))
                .ok_or_else(|| "number overflow".to_string())
        })?
    } else {
        let mut total = 0u64;
        let mut group = String::new();
        let mut last_big = u64::MAX;
        for c in chars {
            let unit = match c {
                '万' => 10_000,
                '億' => 100_000_000,
                '兆' => 1_000_000_000_000,
                _ => {
                    group.push(c);
                    continue;
                }
            };
            if unit >= last_big {
                return Err("large units must strictly descend".into());
            }
            let n = small(&group)?;
            total = total
                .checked_add(n.checked_mul(unit).ok_or("number overflow")?)
                .ok_or("number overflow")?;
            group.clear();
            last_big = unit;
        }
        if !group.is_empty() {
            total = total.checked_add(small(&group)?).ok_or("number overflow")?;
        }
        total
    };
    if value == 0 || value > 9_007_199_254_740_991 {
        Err("number outside positive safe-integer range".into())
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normal_forms() {
        for (input, expected) in [
            ("１", vec![1]),
            ("十二", vec![12]),
            ("一〇一", vec![101]),
            ("百一", vec![101]),
            ("千二百三十四", vec![1234]),
            ("一万二千三", vec![12003]),
            ("一億一万一", vec![100010001]),
            ("二の二", vec![2, 2]),
            ("１２ノ３の四", vec![12, 3, 4]),
        ] {
            assert_eq!(normalize(input).unwrap(), expected, "{input}");
        }
    }
    #[test]
    fn rejects_ambiguous_or_malformed_forms() {
        for input in [
            "",
            "〇",
            "0",
            "の二",
            "二の",
            "二のの三",
            "十十",
            "百千",
            "一二十",
            "一百〇一",
            "1十",
            "壱",
            "万",
            "一万一億",
            "一億億",
            "-1",
            "1.5",
            " 2",
            "18446744073709551616",
        ] {
            assert!(normalize(input).is_err(), "accepted {input}");
        }
    }
}
