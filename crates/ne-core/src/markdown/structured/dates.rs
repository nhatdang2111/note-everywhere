//! Dates as an export writes them in CSV cells, converted to ISO text.
//!
//! Understood: `November 21, 2026`, `Nov 21, 2026`, `November 21, 2026 9:30 AM`,
//! `November 21, 2026 21:30`, a range `<date> → <date>`, an ISO date or
//! date-time, and a trailing time zone note such as `(GMT+7)`, which is
//! dropped because dates here carry no time zone.

use crate::database::{DateValue, is_valid_date};

const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

fn month_number(word: &str) -> Option<u32> {
    let word = word.trim_end_matches('.').to_lowercase();
    if word.len() < 3 {
        return None;
    }
    MONTHS
        .iter()
        .position(|m| m.starts_with(&word))
        .and_then(|i| u32::try_from(i + 1).ok())
}

fn strip_zone(text: &str) -> &str {
    match text.rfind('(') {
        Some(i) if text.trim_end().ends_with(')') => text[..i].trim_end(),
        _ => text,
    }
}

fn parse_time(text: &str) -> Option<(u32, u32)> {
    let lower = text.to_lowercase();
    let (clock, meridiem) = if let Some(t) = lower.strip_suffix("am") {
        (t.trim(), Some(false))
    } else if let Some(t) = lower.strip_suffix("pm") {
        (t.trim(), Some(true))
    } else {
        (lower.trim(), None)
    };
    let (h, m) = clock.split_once(':')?;
    let (mut hour, minute): (u32, u32) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    match meridiem {
        Some(pm) => {
            if !(1..=12).contains(&hour) {
                return None;
            }
            hour %= 12;
            if pm {
                hour += 12;
            }
        }
        None if hour > 23 => return None,
        None => {}
    }
    (minute < 60).then_some((hour, minute))
}

fn parse_one(text: &str) -> Option<String> {
    let text = strip_zone(text.trim()).trim();
    if is_valid_date(text) {
        return Some(text.to_owned());
    }
    // `Month D, YYYY[ time]`
    let mut words = text.split_whitespace();
    let month = month_number(words.next()?)?;
    let day: u32 = words.next()?.trim_end_matches(',').parse().ok()?;
    let year: u32 = words.next()?.trim_end_matches(',').parse().ok()?;
    let rest: Vec<&str> = words.collect();
    let date = format!("{year:04}-{month:02}-{day:02}");
    let iso = if rest.is_empty() {
        date
    } else {
        let (hour, minute) = parse_time(&rest.join(" "))?;
        format!("{date}T{hour:02}:{minute:02}")
    };
    is_valid_date(&iso).then_some(iso)
}

/// Parses a cell into a date, or `None` when it is not one.
pub fn parse_cell_date(text: &str) -> Option<DateValue> {
    let text = text.trim();
    let (start, end) = match text.split_once('→') {
        Some((a, b)) => (parse_one(a)?, Some(parse_one(b)?)),
        None => (parse_one(text)?, None),
    };
    let value = DateValue { start, end };
    value.validate().ok()?;
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_forms_become_iso() {
        let start = |s: &str| parse_cell_date(s).map(|d| d.start);
        assert_eq!(start("November 21, 2026"), Some("2026-11-21".into()));
        assert_eq!(start("Nov 3, 2026"), Some("2026-11-03".into()));
        assert_eq!(
            start("November 21, 2026 9:30 AM"),
            Some("2026-11-21T09:30".into())
        );
        assert_eq!(
            start("November 21, 2026 12:05 AM"),
            Some("2026-11-21T00:05".into())
        );
        assert_eq!(
            start("November 21, 2026 9:30 PM (GMT+7)"),
            Some("2026-11-21T21:30".into())
        );
        assert_eq!(start("2026-11-21"), Some("2026-11-21".into()));
        assert_eq!(
            start("November 21, 2026 21:30"),
            Some("2026-11-21T21:30".into())
        );
    }

    #[test]
    fn ranges_and_rejections() {
        let range = parse_cell_date("November 21, 2026 → November 25, 2026").expect("range");
        assert_eq!(range.end.as_deref(), Some("2026-11-25"));
        assert!(parse_cell_date("November 25, 2026 → November 21, 2026").is_none());
        for bad in [
            "",
            "hello",
            "Someday 3, 2026",
            "February 30, 2026",
            "May 5",
            "May 5, 2026 25:00",
        ] {
            assert!(parse_cell_date(bad).is_none(), "{bad}");
        }
    }
}
