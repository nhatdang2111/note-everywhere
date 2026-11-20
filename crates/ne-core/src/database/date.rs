//! Dates of a date property: ISO text with an optional time and end.
//!
//! A date is the text `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM`, with no time zone:
//! it is the wall-clock date the person sees, wherever they are. ISO text
//! sorts the same as the moments it names, which lets the index sort dates as
//! plain strings. Turning a date into a point in time needs the person's UTC
//! offset, see [`DateValue::start_ms`].

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// A date, optionally with a time of day and an end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateValue {
    /// `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM`.
    pub start: String,
    /// Optional end of a range, in the same two forms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
}

struct Parsed {
    year: i64,
    month: i64,
    day: i64,
    minutes: Option<i64>,
}

fn digits(text: &str, len: usize) -> Option<i64> {
    (text.len() == len && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn parse(text: &str) -> Option<Parsed> {
    let (date, time) = match text.split_once('T') {
        Some((date, time)) => (date, Some(time)),
        None => (text, None),
    };
    let mut parts = date.split('-');
    let year = digits(parts.next()?, 4)?;
    let month = digits(parts.next()?, 2)?;
    let day = digits(parts.next()?, 2)?;
    if parts.next().is_some() || !(1..=12).contains(&month) {
        return None;
    }
    if !(1..=days_in_month(year, month)).contains(&day) {
        return None;
    }
    let minutes = match time {
        None => None,
        Some(time) => {
            let (hour, minute) = time.split_once(':')?;
            let (hour, minute) = (digits(hour, 2)?, digits(minute, 2)?);
            if hour > 23 || minute > 59 {
                return None;
            }
            Some(hour * 60 + minute)
        }
    };
    Some(Parsed {
        year,
        month,
        day,
        minutes,
    })
}

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// True when `text` is a valid date in one of the two forms.
pub fn is_valid_date(text: &str) -> bool {
    parse(text).is_some()
}

/// The `YYYY-MM-DD` day of a valid date text.
pub fn day_of(text: &str) -> Option<&str> {
    parse(text).map(|_| &text[..10])
}

/// The point in time (milliseconds since the Unix epoch) of a date text, for a
/// person whose clock is `utc_offset_minutes` ahead of UTC. A date without a
/// time is taken as `default_minutes` after midnight.
pub fn to_epoch_ms(text: &str, utc_offset_minutes: i64, default_minutes: i64) -> Option<i64> {
    let parsed = parse(text)?;
    let days = days_from_civil(parsed.year, parsed.month, parsed.day);
    let minutes = days * 24 * 60 + parsed.minutes.unwrap_or(default_minutes) - utc_offset_minutes;
    Some(minutes * 60_000)
}

impl DateValue {
    /// A date without an end.
    pub fn new(start: impl Into<String>) -> Self {
        Self {
            start: start.into(),
            end: None,
        }
    }

    /// Checks both ends are valid dates and the end is not before the start.
    pub fn validate(&self) -> Result<()> {
        if !is_valid_date(&self.start) {
            return Err(Error::Invalid(format!("{:?} is not a date", self.start)));
        }
        if let Some(end) = &self.end {
            if !is_valid_date(end) {
                return Err(Error::Invalid(format!("{end:?} is not a date")));
            }
            if end.as_str() < self.start.as_str() {
                return Err(Error::Invalid(
                    "the end of a date is before its start".into(),
                ));
            }
        }
        Ok(())
    }

    /// The point in time the date starts, see [`to_epoch_ms`].
    pub fn start_ms(&self, utc_offset_minutes: i64, default_minutes: i64) -> Option<i64> {
        to_epoch_ms(&self.start, utc_offset_minutes, default_minutes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_real_dates() {
        assert!(is_valid_date("2026-11-21"));
        assert!(is_valid_date("2024-02-29"));
        assert!(is_valid_date("2026-11-21T09:30"));
        for bad in [
            "2026-02-29",
            "2026-13-01",
            "2026-00-10",
            "2026-11-31",
            "2026-11-21T24:00",
            "2026-11-21T9:30",
            "26-11-21",
            "2026-11-21 09:30",
            "",
        ] {
            assert!(!is_valid_date(bad), "{bad}");
        }
    }

    #[test]
    fn epoch_conversion_matches_known_moments() {
        assert_eq!(to_epoch_ms("1970-01-01", 0, 0), Some(0));
        // 2026-11-21 09:00 in Vietnam (UTC+7) is 02:00 UTC.
        assert_eq!(
            to_epoch_ms("2026-11-21", 420, 9 * 60),
            to_epoch_ms("2026-11-21T02:00", 0, 0)
        );
        assert_eq!(to_epoch_ms("2000-03-01T00:00", 0, 0), Some(951_868_800_000));
        assert_eq!(to_epoch_ms("1969-12-31T23:00", 0, 0), Some(-3_600_000));
    }

    #[test]
    fn end_before_start_is_rejected() {
        let mut value = DateValue::new("2026-11-21");
        assert!(value.validate().is_ok());
        value.end = Some("2026-11-20".into());
        assert!(value.validate().is_err());
        value.end = Some("2026-11-22T10:00".into());
        assert!(value.validate().is_ok());
    }

    #[test]
    fn iso_text_sorts_like_time() {
        let mut dates = ["2026-11-21T09:00", "2026-11-21", "2026-11-20T23:59"];
        dates.sort_unstable();
        assert_eq!(
            dates,
            ["2026-11-20T23:59", "2026-11-21", "2026-11-21T09:00"]
        );
    }
}
