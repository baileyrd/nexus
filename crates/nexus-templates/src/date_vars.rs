//! `{{date...}}` dynamic variable mini-language (#367 / C14).
//!
//! `{{today}}`/`{{now}}` (see [`crate::template`]) are fixed-format,
//! zero-argument built-ins. This module adds a small family layered on
//! top, all sharing the `date` prefix so they don't collide with a
//! template's own declared parameters unless one is literally named
//! `date...`:
//!
//! - `{{date}}` — today, `YYYY-MM-DD`, local time.
//! - `{{date:FORMAT}}` — today in a custom format (see [`format_local`]
//!   for the supported tokens).
//! - `{{date+Nd}}` / `{{date-Nd}}` — N days from/before today (unit `d`
//!   days, `w` weeks, `h` hours), default format.
//! - `{{date+Nd:FORMAT}}` — offset + custom format combined, e.g.
//!   `{{date+7d:YYYY-MM-DD}}` for a next-week daily-note link.
//!
//! Intentionally tiny, matching [`crate::substitute`]'s "no
//! conditionals, no loops" posture — this is a fixed grammar, not an
//! expression evaluator.

use chrono::{DateTime, Datelike, Duration, Local, Timelike};

/// Resolve `name` (the trimmed text between `{{` and `}}`, e.g.
/// `"date+7d:YYYY-MM-DD"`) as a dynamic date variable against `now`.
/// Returns `None` when `name` doesn't match the `date...` grammar at
/// all — callers should leave those tags for the normal
/// builtin/parameter/user-arg lookup.
#[must_use]
pub fn resolve(name: &str, now: DateTime<Local>) -> Option<String> {
    let rest = name.strip_prefix("date")?;
    let (offset, rest) = parse_offset(rest)?;
    let format = match rest.strip_prefix(':') {
        Some(f) if !f.is_empty() => f,
        None if rest.is_empty() => "YYYY-MM-DD",
        // "date:" with nothing after the colon, or trailing garbage
        // that isn't `:FORMAT` — not a valid date var.
        _ => return None,
    };
    Some(format_local(now + offset, format))
}

/// Parse an optional leading `+N<unit>` / `-N<unit>` off `rest`,
/// returning the offset (zero if none was present) and the remaining
/// unparsed suffix. `unit` is `d` (days), `w` (weeks), or `h` (hours).
fn parse_offset(rest: &str) -> Option<(Duration, &str)> {
    let negative = rest.starts_with('-');
    let Some(sign_rest) = rest.strip_prefix('+').or_else(|| rest.strip_prefix('-')) else {
        return Some((Duration::zero(), rest));
    };
    let digits_end = sign_rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(sign_rest.len());
    if digits_end == 0 {
        return None; // `+`/`-` with no digits — not a valid offset
    }
    let n: i64 = sign_rest[..digits_end].parse().ok()?;
    let n = if negative { -n } else { n };
    let mut chars = sign_rest[digits_end..].chars();
    let unit = chars.next()?;
    let duration = match unit {
        'd' => Duration::days(n),
        'w' => Duration::weeks(n),
        'h' => Duration::hours(n),
        _ => return None,
    };
    Some((duration, chars.as_str()))
}

/// Render `dt` against a small Templater-style token format string.
/// Supported tokens (case-sensitive, matched longest-first so `YYYY`
/// wins over `YY`): `YYYY`, `YY`, `MM`, `DD`, `HH`, `mm`, `ss`. Every
/// other character (including stray `%`) passes through literally —
/// this isn't `strftime`, so there's no escape mechanism to trip over.
#[must_use]
pub fn format_local(dt: DateTime<Local>, format: &str) -> String {
    type TokenFn = fn(&DateTime<Local>) -> String;
    const TOKENS: &[(&str, TokenFn)] = &[
        ("YYYY", |d| format!("{:04}", d.year())),
        ("YY", |d| format!("{:02}", d.year().rem_euclid(100))),
        ("MM", |d| format!("{:02}", d.month())),
        ("DD", |d| format!("{:02}", d.day())),
        ("HH", |d| format!("{:02}", d.hour())),
        ("mm", |d| format!("{:02}", d.minute())),
        ("ss", |d| format!("{:02}", d.second())),
    ];
    let mut out = String::with_capacity(format.len());
    let mut rest = format;
    'outer: while let Some(ch) = rest.chars().next() {
        for (token, render) in TOKENS {
            if let Some(after) = rest.strip_prefix(token) {
                out.push_str(&render(&dt));
                rest = after;
                continue 'outer;
            }
        }
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
    }

    #[test]
    fn bare_date_defaults_to_yyyy_mm_dd() {
        assert_eq!(resolve("date", at(2026, 3, 5, 10, 0, 0)).unwrap(), "2026-03-05");
    }

    #[test]
    fn custom_format() {
        assert_eq!(
            resolve("date:MM/DD/YYYY", at(2026, 3, 5, 10, 0, 0)).unwrap(),
            "03/05/2026"
        );
    }

    #[test]
    fn positive_day_offset() {
        assert_eq!(resolve("date+7d", at(2026, 3, 5, 0, 0, 0)).unwrap(), "2026-03-12");
    }

    #[test]
    fn negative_day_offset() {
        assert_eq!(resolve("date-1d", at(2026, 3, 5, 0, 0, 0)).unwrap(), "2026-03-04");
    }

    #[test]
    fn week_offset() {
        assert_eq!(resolve("date+1w", at(2026, 3, 5, 0, 0, 0)).unwrap(), "2026-03-12");
    }

    #[test]
    fn hour_offset_with_time_format() {
        assert_eq!(
            resolve("date+2h:HH:mm", at(2026, 3, 5, 22, 30, 0)).unwrap(),
            "00:30"
        );
    }

    #[test]
    fn offset_and_format_combined() {
        assert_eq!(
            resolve("date+7d:YYYY-MM-DD", at(2026, 3, 5, 0, 0, 0)).unwrap(),
            "2026-03-12"
        );
    }

    #[test]
    fn month_crossing_offset() {
        assert_eq!(resolve("date+30d", at(2026, 1, 15, 0, 0, 0)).unwrap(), "2026-02-14");
    }

    #[test]
    fn non_date_prefix_returns_none() {
        assert_eq!(resolve("today", at(2026, 3, 5, 0, 0, 0)), None);
        assert_eq!(resolve("title", at(2026, 3, 5, 0, 0, 0)), None);
    }

    #[test]
    fn malformed_offset_returns_none() {
        assert_eq!(resolve("date+", at(2026, 3, 5, 0, 0, 0)), None);
        assert_eq!(resolve("date+7", at(2026, 3, 5, 0, 0, 0)), None); // missing unit
        assert_eq!(resolve("date+7x", at(2026, 3, 5, 0, 0, 0)), None); // unknown unit
    }

    #[test]
    fn trailing_garbage_after_offset_returns_none() {
        assert_eq!(resolve("date+7dxyz", at(2026, 3, 5, 0, 0, 0)), None);
    }

    #[test]
    fn empty_format_after_colon_returns_none() {
        assert_eq!(resolve("date:", at(2026, 3, 5, 0, 0, 0)), None);
    }

    #[test]
    fn literal_characters_pass_through_format() {
        assert_eq!(
            format_local(at(2026, 3, 5, 9, 5, 3), "YYYY/MM/DD @ HH:mm:ss"),
            "2026/03/05 @ 09:05:03"
        );
    }
}
