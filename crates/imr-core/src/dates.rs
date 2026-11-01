//! Reading dates from text and Excel serial numbers.
//!
//! The Python version used pandas' free-form parser (dateutil). This accepts the formats that
//! show up in workbooks: ISO dates and times, US `month/day/year` (or `day/month/year` when the
//! first number can't be a month), two-digit years, month names, a leading weekday, and
//! `January 2024` (the 1st). It does not accept relative words (`today`) or partial dates that
//! borrow the current year.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime};

/// Excel's day zero for serial dates (accounting for its 1900 leap-year bug).
pub fn excel_epoch() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(1899, 12, 30).expect("valid date").and_time(NaiveTime::MIN)
}

/// The largest serial Excel accepts (9999-12-31).
pub const MAX_SERIAL: f64 = 2_958_465.0;

/// An Excel serial day number (fractions are times of day) as a timestamp.
pub fn from_serial(days: f64) -> Option<NaiveDateTime> {
    if !(1.0..=MAX_SERIAL).contains(&days) {
        return None;
    }
    let nanos = (days * 86_400e9).round() as i64;
    excel_epoch().checked_add_signed(Duration::nanoseconds(nanos))
}

const DATE_FORMATS: &[&str] = &[
    "%Y-%m-%d",
    "%Y/%m/%d",
    "%Y.%m.%d",
    "%m/%d/%Y",
    "%m-%d-%Y",
    "%m.%d.%Y",
    "%d/%m/%Y",
    "%d-%m-%Y",
    "%m/%d/%y",
    "%m-%d-%y",
    "%b %d %Y",
    "%b %d, %Y",
    "%B %d %Y",
    "%B %d, %Y",
    "%d %b %Y",
    "%d %B %Y",
    "%d-%b-%Y",
    "%d-%b-%y",
    "%b %d,%Y",
    "%Y%m%d",
];

const TIME_FORMATS: &[&str] =
    &["%H:%M:%S%.f", "%H:%M:%S", "%H:%M", "%I:%M:%S %p", "%I:%M %p", "%I:%M:%S%p", "%I:%M%p"];

const WEEKDAYS: &[&str] = &[
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "mon",
    "tue",
    "wed",
    "thu",
    "fri",
    "sat",
    "sun",
];

/// `Monday, January 8, 2024` -> `January 8, 2024`.
fn strip_weekday(s: &str) -> &str {
    let lower = s.to_ascii_lowercase();
    for w in WEEKDAYS {
        if lower.starts_with(w) {
            let rest = &s[w.len()..];
            if rest.starts_with([',', ' ']) {
                return rest.trim_start_matches([',', ' ']);
            }
        }
    }
    s
}

/// A date in one of [`DATE_FORMATS`]; a four-digit-year format doesn't take a two-digit year.
fn parse_date_with(s: &str, f: &str) -> Option<NaiveDate> {
    let d = NaiveDate::parse_from_str(s, f).ok()?;
    if f.contains("%Y") && d.year() < 100 {
        return None;
    }
    Some(d)
}

/// `January 2024` or `Jan 2024`: the first of that month.
fn month_year(s: &str) -> Option<NaiveDate> {
    let (month, year) = s.split_once(' ')?;
    if year.len() != 4 || !year.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let text = format!("{} 1 {year}", month.trim_end_matches('.'));
    ["%B %d %Y", "%b %d %Y"].iter().find_map(|f| NaiveDate::parse_from_str(&text, f).ok())
}

/// A date or date-time written as text, or None when it isn't one.
pub fn parse_datetime(text: &str) -> Option<NaiveDateTime> {
    let s = strip_weekday(text.trim());
    if s.len() < 6 || !s.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    for f in DATE_FORMATS {
        if *f == "%Y%m%d" && !(s.len() == 8 && s.bytes().all(|b| b.is_ascii_digit())) {
            continue;
        }
        if let Some(d) = parse_date_with(s, f) {
            return Some(d.and_time(NaiveTime::MIN));
        }
    }
    if let Some(d) = month_year(s) {
        return Some(d.and_time(NaiveTime::MIN));
    }
    // Date and time separated by a space or a T.
    let split = s.find('T').filter(|&i| i >= 8).or_else(|| s.find(' ').filter(|&i| i >= 6));
    if let Some(i) = split {
        let (date_part, time_part) = (&s[..i], s[i + 1..].trim());
        // "Jan 5 2024 13:00": the date itself may contain spaces, so try every split point.
        if let Some(d) = parse_date_only(date_part)
            && let Some(t) = parse_time(time_part)
        {
            return Some(d.and_time(t));
        }
    }
    for (i, _) in s.match_indices(' ') {
        if let (Some(d), Some(t)) = (parse_date_only(&s[..i]), parse_time(s[i + 1..].trim())) {
            return Some(d.and_time(t));
        }
    }
    None
}

fn parse_date_only(s: &str) -> Option<NaiveDate> {
    DATE_FORMATS.iter().filter(|f| **f != "%Y%m%d").find_map(|f| parse_date_with(s.trim(), f))
}

fn parse_time(s: &str) -> Option<NaiveTime> {
    let s = s.trim_end_matches('Z');
    TIME_FORMATS.iter().find_map(|f| NaiveTime::parse_from_str(s, f).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    #[test]
    fn formats() {
        assert_eq!(parse_datetime("2024-01-05"), Some(dt("2024-01-05 00:00:00")));
        assert_eq!(parse_datetime("1/5/2024"), Some(dt("2024-01-05 00:00:00")));
        assert_eq!(parse_datetime("01/06/2024"), Some(dt("2024-01-06 00:00:00")));
        assert_eq!(parse_datetime("Jan 7, 2024"), Some(dt("2024-01-07 00:00:00")));
        assert_eq!(parse_datetime("2024-01-05 13:00"), Some(dt("2024-01-05 13:00:00")));
        assert_eq!(parse_datetime("2024-01-05T13:00:05"), Some(dt("2024-01-05 13:00:05")));
        assert_eq!(parse_datetime("Jan 7 2024 1:30 PM"), Some(dt("2024-01-07 13:30:00")));
        assert_eq!(parse_datetime("01/15/24"), Some(dt("2024-01-15 00:00:00")));
        assert_eq!(parse_datetime("13/01/2024"), Some(dt("2024-01-13 00:00:00")));
        assert_eq!(parse_datetime("Monday, January 8, 2024"), Some(dt("2024-01-08 00:00:00")));
        assert_eq!(parse_datetime("January 2024"), Some(dt("2024-01-01 00:00:00")));
        assert_eq!(parse_datetime("2024-13-01"), None);
        assert_eq!(parse_datetime("pending"), None);
        assert_eq!(parse_datetime("1 Micrococcus luteus"), None);
    }

    #[test]
    fn serials() {
        assert_eq!(from_serial(45292.0), Some(dt("2024-01-01 00:00:00")));
        assert_eq!(from_serial(45292.5), Some(dt("2024-01-01 12:00:00")));
        assert_eq!(from_serial(0.5), None);
    }
}
