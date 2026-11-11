//! TimePoint arithmetic (TIME-02, TIME-03). Proleptic Gregorian calendar.
use serde_json::Value;

/// A civil date.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub y: i64,
    pub m: u32,
    pub d: u32,
}

fn leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn month_len(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if leap(y) => 29,
        _ => 28,
    }
}

/// A day of the month or a month number, which the arithmetic keeps in 0..=31.
fn small(n: i64) -> u32 {
    u32::try_from(n).expect("a day or month computed from a day count")
}

impl Date {
    /// Parse an existing `YYYY-MM-DD` date (year 0 is not a civil year).
    pub fn parse(s: &str) -> Option<Date> {
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        let num = |r: std::ops::Range<usize>| -> Option<u32> {
            let t = &s[r];
            t.bytes()
                .all(|c| c.is_ascii_digit())
                .then(|| t.parse().ok())?
        };
        let (y, m, d) = (i64::from(num(0..4)?), num(5..7)?, num(8..10)?);
        (y >= 1 && (1..=12).contains(&m) && d >= 1 && d <= month_len(y, m)).then_some(Date {
            y,
            m,
            d,
        })
    }
    fn days(self) -> i64 {
        // Days from civil (H. Hinnant).
        let y = if self.m <= 2 { self.y - 1 } else { self.y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = i64::from(self.m);
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(self.d) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146097 + doe
    }
    fn from_days(z: i64) -> Date {
        let era = z.div_euclid(146097);
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        // `doy` is in 0..=365 and `mp` in 0..=11, so the day is in 1..=31 and
        // the month in 1..=12.
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
        Date {
            y,
            m: small(m),
            d: small(d),
        }
    }
    pub fn add_days(self, n: i64) -> Date {
        Date::from_days(self.days() + n)
    }
    /// Civil Code art. 143 counting from `self` as the first day: the day after
    /// the period of `n` months ends is the corresponding day `n` months later,
    /// or the first day of the following month when that day does not exist.
    pub fn after_months(self, n: i64) -> Date {
        let total = self.y * 12 + (i64::from(self.m) - 1) + n;
        let (y, m) = (total.div_euclid(12), small(total.rem_euclid(12)) + 1);
        if self.d <= month_len(y, m) {
            Date { y, m, d: self.d }
        } else {
            Date { y, m, d: 1 }.add_days(i64::from(month_len(y, m)))
        }
    }
    pub fn to_iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }
}

/// The fixed day of a TimePoint, if it has one: a day-precision `date`, or a
/// `derived` whose value is fixed by its base (TIME-03).
pub fn fixed_day(tp: &Value) -> Option<Date> {
    match tp["state"].as_str()? {
        "date" if tp["precision"] == "day" => Date::parse(tp["value"].as_str()?),
        "derived" if fixed_day(&tp["base"]).is_some() => Date::parse(tp["value"].as_str()?),
        _ => None,
    }
}

/// Evaluate `calc` on a fixed base day.
pub fn calc(calc: &Value, base: Date) -> Option<Date> {
    match calc["op"].as_str()? {
        "sameDay" => Some(base),
        "nextDay" => Some(base.add_days(1)),
        "afterElapsed" => {
            let n = calc["count"].as_i64()?;
            match calc["unit"].as_str()? {
                "day" => Some(base.add_days(n)),
                "month" => Some(base.after_months(n)),
                "year" => Some(base.after_months(n.checked_mul(12)?)),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(s: &str) -> Date {
        Date::parse(s).unwrap()
    }
    #[test]
    fn calendar() {
        assert!(Date::parse("2023-02-29").is_none());
        assert!(Date::parse("2024-02-29").is_some());
        assert!(Date::parse("0000-01-01").is_none());
        assert_eq!(d("2019-04-30").add_days(1), d("2019-05-01"));
        assert_eq!(d("1999-12-31").add_days(1), d("2000-01-01"));
        assert_eq!(d("2020-01-01").add_days(30), d("2020-01-31"));
        assert_eq!(d("2020-01-15").after_months(6), d("2020-07-15"));
        assert_eq!(d("2020-01-31").after_months(1), d("2020-03-01"));
        assert_eq!(d("2019-01-31").after_months(1), d("2019-03-01"));
        assert_eq!(d("2020-02-29").after_months(12), d("2021-03-01"));
        assert_eq!(d("2020-12-10").after_months(2), d("2021-02-10"));
        for s in ["1873-01-01", "1600-02-29", "2400-12-31"] {
            assert_eq!(d(s).add_days(0).to_iso(), s);
        }
    }

    #[test]
    fn parse_takes_ten_ascii_characters() {
        for bad in [
            "2020-1-01",
            "2020-01-1",
            "2020/01/01",
            "20200101",
            "2020-13-01",
            "2020-00-10",
            "2020-04-31",
            "2020-01-00",
            "+020-01-01",
            "2020-+1-01",
            "２０２０-01-01",
            "2020-01-01 ",
            "",
        ] {
            assert!(Date::parse(bad).is_none(), "{bad:?}");
        }
        assert_eq!(d("0001-01-01"), Date { y: 1, m: 1, d: 1 });
        assert_eq!(
            d("9999-12-31"),
            Date {
                y: 9999,
                m: 12,
                d: 31
            }
        );
    }

    #[test]
    fn days_and_months_cross_every_boundary() {
        // Every day of a leap year and of the years around a century.
        for start in ["2023-12-31", "1899-12-31", "1999-12-31"] {
            let mut day = d(start);
            for _ in 0..800 {
                let next = day.add_days(1);
                assert!(next > day);
                assert_eq!(next.add_days(-1), day);
                assert_eq!(Date::parse(&next.to_iso()), Some(next));
                day = next;
            }
        }
        assert_eq!(d("2024-02-28").add_days(1), d("2024-02-29"));
        assert_eq!(d("2100-02-28").add_days(1), d("2100-03-01"));
        assert_eq!(d("2000-03-01").add_days(-1), d("2000-02-29"));
        assert_eq!(d("0001-01-01").add_days(-1).to_iso(), "0000-12-31");
        // Months: across a year end, backwards, and onto a shorter month.
        assert_eq!(d("2020-11-30").after_months(3), d("2021-03-01"));
        assert_eq!(d("2020-11-30").after_months(15), d("2022-03-01"));
        assert_eq!(d("2020-03-31").after_months(-1), d("2020-03-01"));
        assert_eq!(d("2020-03-15").after_months(-3), d("2019-12-15"));
        assert_eq!(d("2020-01-15").after_months(-1), d("2019-12-15"));
        assert_eq!(d("2020-01-15").after_months(0), d("2020-01-15"));
        assert_eq!(d("2023-01-31").after_months(13), d("2024-03-01"));
        assert_eq!(d("2023-01-29").after_months(13), d("2024-02-29"));
    }
}
