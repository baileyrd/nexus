//! Minimal cron-expression parser + next-fire calculator.
//!
//! Supports the classic 5-field syntax `minute hour day-of-month
//! month day-of-week` with per-field wildcard (`*`), single value
//! (`15`), comma list (`0,15,30,45`), range (`9-17`), and step
//! (`*/5`, `9-17/2`). Day-of-week uses the standard `0=Sunday`
//! convention; names like `mon` / `jan` are not supported.
//!
//! No external crate — chrono provides calendar math, the per-field
//! matcher is a `BTreeSet<u32>`.

use std::collections::BTreeSet;

use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use thiserror::Error;

/// Parsed cron schedule. Five field matchers, each an allowed set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronSchedule {
    minute: BTreeSet<u32>,
    hour: BTreeSet<u32>,
    day_of_month: BTreeSet<u32>,
    month: BTreeSet<u32>,
    day_of_week: BTreeSet<u32>,
}

/// Parse errors.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CronParseError {
    /// Wrong number of whitespace-separated fields.
    #[error("cron needs 5 fields, got {0}")]
    WrongFieldCount(usize),
    /// A field had a token we don't understand or a value out of range.
    #[error("cron field '{field}' is invalid: {reason}")]
    InvalidField {
        /// Which field (`minute` / `hour` / etc).
        field: &'static str,
        /// Explanation.
        reason: String,
    },
}

impl CronSchedule {
    /// Parse a 5-field cron expression.
    ///
    /// # Errors
    /// [`CronParseError::WrongFieldCount`] when the expression has
    /// more or fewer than five fields, or
    /// [`CronParseError::InvalidField`] when a field can't be parsed.
    pub fn parse(expr: &str) -> Result<Self, CronParseError> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(CronParseError::WrongFieldCount(fields.len()));
        }
        Ok(Self {
            minute: parse_field("minute", fields[0], 0, 59)?,
            hour: parse_field("hour", fields[1], 0, 23)?,
            day_of_month: parse_field("day-of-month", fields[2], 1, 31)?,
            month: parse_field("month", fields[3], 1, 12)?,
            day_of_week: parse_field("day-of-week", fields[4], 0, 6)?,
        })
    }

    /// First instant strictly after `from` that matches the schedule.
    /// Returns `None` if no match is found within ~366 days — which
    /// only happens for intrinsically-impossible expressions (e.g.
    /// `0 0 30 2 *`). Minute-granularity; seconds are always `0`.
    #[must_use]
    pub fn next_after(&self, from: DateTime<Utc>) -> Option<DateTime<Utc>> {
        // Start at the next full minute after `from` so a call made
        // during a matching minute still advances. Zero-out seconds /
        // nanos so comparisons are minute-aligned.
        let start = (from + Duration::minutes(1))
            .with_second(0)?
            .with_nanosecond(0)?;
        let mut cursor = start;
        for _ in 0..(366 * 24 * 60) {
            if self.matches(cursor) {
                return Some(cursor);
            }
            cursor += Duration::minutes(1);
        }
        None
    }

    fn matches(&self, dt: DateTime<Utc>) -> bool {
        let minute = dt.minute();
        let hour = dt.hour();
        let dom = dt.day();
        let month = dt.month();
        // chrono: weekday Mon=1..Sun=7 via `.number_from_monday`;
        // the standard cron convention is Sun=0..Sat=6.
        let dow_chrono = dt.weekday().num_days_from_sunday();

        if !self.minute.contains(&minute) || !self.hour.contains(&hour) {
            return false;
        }
        if !self.month.contains(&month) {
            return false;
        }
        // Per POSIX cron: if BOTH `day-of-month` and `day-of-week`
        // are restricted (non-`*`), match when EITHER matches.
        // Otherwise require the one that's restricted to match.
        let dom_restricted = !is_full_range(&self.day_of_month, 1, 31);
        let dow_restricted = !is_full_range(&self.day_of_week, 0, 6);
        let dom_ok = self.day_of_month.contains(&dom);
        let dow_ok = self.day_of_week.contains(&dow_chrono);
        match (dom_restricted, dow_restricted) {
            (true, true) => dom_ok || dow_ok,
            (true, false) => dom_ok,
            (false, true) => dow_ok,
            (false, false) => true,
        }
    }
}

fn is_full_range(set: &BTreeSet<u32>, lo: u32, hi: u32) -> bool {
    set.len() as u32 == hi - lo + 1
}

fn parse_field(
    name: &'static str,
    raw: &str,
    lo: u32,
    hi: u32,
) -> Result<BTreeSet<u32>, CronParseError> {
    let mut out = BTreeSet::new();
    for piece in raw.split(',') {
        let piece = piece.trim();
        if piece.is_empty() {
            return invalid(name, "empty token");
        }
        // Split step `A/B`.
        let (range_part, step) = match piece.split_once('/') {
            Some((r, s)) => {
                let step: u32 = s
                    .parse()
                    .map_err(|_| invalid_err(name, format!("bad step '{s}'")))?;
                if step == 0 {
                    return invalid(name, "step cannot be 0");
                }
                (r, step)
            }
            None => (piece, 1u32),
        };
        let (start, end) = parse_range(name, range_part, lo, hi)?;
        let mut v = start;
        while v <= end {
            out.insert(v);
            v += step;
        }
    }
    if out.is_empty() {
        return invalid(name, "no values matched");
    }
    Ok(out)
}

fn parse_range(
    name: &'static str,
    raw: &str,
    lo: u32,
    hi: u32,
) -> Result<(u32, u32), CronParseError> {
    if raw == "*" {
        return Ok((lo, hi));
    }
    if let Some((a, b)) = raw.split_once('-') {
        let start: u32 = a
            .parse()
            .map_err(|_| invalid_err(name, format!("bad range start '{a}'")))?;
        let end: u32 = b
            .parse()
            .map_err(|_| invalid_err(name, format!("bad range end '{b}'")))?;
        if start < lo || end > hi || start > end {
            return invalid(name, &format!("range {start}-{end} out of {lo}..={hi}"));
        }
        return Ok((start, end));
    }
    let v: u32 = raw
        .parse()
        .map_err(|_| invalid_err(name, format!("bad value '{raw}'")))?;
    if v < lo || v > hi {
        return invalid(name, &format!("value {v} out of {lo}..={hi}"));
    }
    Ok((v, v))
}

fn invalid<T>(field: &'static str, reason: &str) -> Result<T, CronParseError> {
    Err(CronParseError::InvalidField {
        field,
        reason: reason.to_string(),
    })
}

fn invalid_err(field: &'static str, reason: String) -> CronParseError {
    CronParseError::InvalidField { field, reason }
}

/// Convenience: parse + compute in one call. Useful when the caller
/// just wants the next fire time from a raw string.
///
/// # Errors
/// Propagates [`CronParseError`] from [`CronSchedule::parse`].
pub fn next_fire_after(
    expr: &str,
    from: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, CronParseError> {
    Ok(CronSchedule::parse(expr)?.next_after(from))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, mo: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, min, 0).unwrap()
    }

    #[test]
    fn parses_basic_daily() {
        let s = CronSchedule::parse("0 9 * * *").unwrap();
        assert_eq!(s.minute, [0].iter().copied().collect());
        assert_eq!(s.hour, [9].iter().copied().collect());
    }

    #[test]
    fn every_minute_any_hour() {
        let s = CronSchedule::parse("* * * * *").unwrap();
        let next = s.next_after(at(2026, 4, 18, 12, 0)).unwrap();
        assert_eq!(next, at(2026, 4, 18, 12, 1));
    }

    #[test]
    fn nine_am_daily_advances_to_next_day_after_noon() {
        let s = CronSchedule::parse("0 9 * * *").unwrap();
        let next = s.next_after(at(2026, 4, 18, 12, 0)).unwrap();
        assert_eq!(next, at(2026, 4, 19, 9, 0));
    }

    #[test]
    fn nine_am_daily_fires_same_day_if_before_nine() {
        let s = CronSchedule::parse("0 9 * * *").unwrap();
        let next = s.next_after(at(2026, 4, 18, 6, 0)).unwrap();
        assert_eq!(next, at(2026, 4, 18, 9, 0));
    }

    #[test]
    fn every_fifteen_minutes_step() {
        let s = CronSchedule::parse("*/15 * * * *").unwrap();
        let next = s.next_after(at(2026, 4, 18, 12, 3)).unwrap();
        assert_eq!(next, at(2026, 4, 18, 12, 15));
    }

    #[test]
    fn range_hours_with_step() {
        // every hour from 9-17 on the 0th minute
        let s = CronSchedule::parse("0 9-17/2 * * *").unwrap();
        assert!(s.hour.contains(&9));
        assert!(s.hour.contains(&11));
        assert!(s.hour.contains(&13));
        assert!(s.hour.contains(&15));
        assert!(s.hour.contains(&17));
        assert!(!s.hour.contains(&10));
    }

    #[test]
    fn weekdays_only() {
        // mon-fri at 9am
        let s = CronSchedule::parse("0 9 * * 1-5").unwrap();
        // 2026-04-18 is a Saturday; next fire should be Monday 4/20.
        let next = s.next_after(at(2026, 4, 18, 0, 0)).unwrap();
        assert_eq!(next, at(2026, 4, 20, 9, 0));
    }

    #[test]
    fn comma_list() {
        let s = CronSchedule::parse("0,30 * * * *").unwrap();
        let next = s.next_after(at(2026, 4, 18, 12, 15)).unwrap();
        assert_eq!(next, at(2026, 4, 18, 12, 30));
    }

    #[test]
    fn invalid_field_count() {
        let err = CronSchedule::parse("0 9 * *").unwrap_err();
        assert!(matches!(err, CronParseError::WrongFieldCount(4)));
    }

    #[test]
    fn invalid_step_zero() {
        let err = CronSchedule::parse("*/0 * * * *").unwrap_err();
        assert!(matches!(err, CronParseError::InvalidField { .. }));
    }

    #[test]
    fn out_of_range_minute() {
        let err = CronSchedule::parse("60 * * * *").unwrap_err();
        assert!(matches!(err, CronParseError::InvalidField { .. }));
    }

    #[test]
    fn dom_or_dow_posix_semantics() {
        // Fire on the 1st of the month OR any Monday.
        let s = CronSchedule::parse("0 0 1 * 1").unwrap();
        // 2026-04-05 is a Sunday; next should be Monday 4/6 at 00:00.
        let next = s.next_after(at(2026, 4, 5, 12, 0)).unwrap();
        assert_eq!(next, at(2026, 4, 6, 0, 0));
        // From 2026-04-28 (Tuesday), next should be 5/1 (1st of month).
        let next = s.next_after(at(2026, 4, 28, 1, 0)).unwrap();
        assert_eq!(next, at(2026, 5, 1, 0, 0));
    }

    #[test]
    fn impossible_expression_returns_none_eventually() {
        // Feb 30 never happens.
        let s = CronSchedule::parse("0 0 30 2 *").unwrap();
        assert_eq!(s.next_after(at(2026, 4, 18, 12, 0)), None);
    }
}
