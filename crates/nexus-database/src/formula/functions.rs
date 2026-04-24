//! Built-in function library for the formula language.
//!
//! Functions are dispatched by name from the evaluator. Each function
//! validates its argument count and types, returning a descriptive error
//! on mismatch.

use crate::error::{DatabaseError, Result};
use crate::formula::eval::FormulaValue;

/// Call a built-in function by name with evaluated arguments.
///
/// # Errors
///
/// Returns `DatabaseError::FormulaError` for unknown functions or
/// argument type/count mismatches.
#[allow(clippy::too_many_lines)]
pub fn call(name: &str, args: &[FormulaValue]) -> Result<FormulaValue> {
    match name {
        // ── String functions ────────────────────────────────────────────
        "concat" => {
            let parts: Vec<String> = args.iter().map(FormulaValue::to_display_string).collect();
            Ok(FormulaValue::String(parts.concat()))
        }
        "upper" => {
            check_args(name, args, 1)?;
            Ok(FormulaValue::String(
                args[0].to_display_string().to_uppercase(),
            ))
        }
        "lower" => {
            check_args(name, args, 1)?;
            Ok(FormulaValue::String(
                args[0].to_display_string().to_lowercase(),
            ))
        }
        "trim" => {
            check_args(name, args, 1)?;
            Ok(FormulaValue::String(
                args[0].to_display_string().trim().to_string(),
            ))
        }
        #[allow(clippy::cast_precision_loss)]
        "len" | "length" => {
            check_args(name, args, 1)?;
            match &args[0] {
                FormulaValue::String(s) => Ok(FormulaValue::Number(s.len() as f64)),
                FormulaValue::Array(a) => Ok(FormulaValue::Number(a.len() as f64)),
                _ => Ok(FormulaValue::Number(
                    args[0].to_display_string().len() as f64,
                )),
            }
        }
        "replace" => {
            check_args(name, args, 3)?;
            let s = args[0].to_display_string();
            let from = args[1].to_display_string();
            let to = args[2].to_display_string();
            Ok(FormulaValue::String(s.replace(&from, &to)))
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        "slice" => {
            check_args_range(name, args, 2, 3)?;
            let s = args[0].to_display_string();
            let start = args[1]
                .as_number()
                .map_or(0, |n| n as usize)
                .min(s.len());
            let end = args
                .get(2)
                .and_then(FormulaValue::as_number)
                .map_or(s.len(), |n| (n as usize).min(s.len()));
            Ok(FormulaValue::String(s[start..end].to_string()))
        }
        "contains" => {
            check_args(name, args, 2)?;
            let haystack = args[0].to_display_string();
            let needle = args[1].to_display_string();
            Ok(FormulaValue::Boolean(haystack.contains(&needle)))
        }
        "starts_with" => {
            check_args(name, args, 2)?;
            Ok(FormulaValue::Boolean(
                args[0]
                    .to_display_string()
                    .starts_with(&args[1].to_display_string()),
            ))
        }
        "ends_with" => {
            check_args(name, args, 2)?;
            Ok(FormulaValue::Boolean(
                args[0]
                    .to_display_string()
                    .ends_with(&args[1].to_display_string()),
            ))
        }

        // ── Numeric functions ───────────────────────────────────────────
        "abs" => {
            check_args(name, args, 1)?;
            let n = require_number(name, &args[0])?;
            Ok(FormulaValue::Number(n.abs()))
        }
        #[allow(clippy::cast_possible_truncation)]
        "round" => {
            check_args_range(name, args, 1, 2)?;
            let n = require_number(name, &args[0])?;
            let places = args
                .get(1)
                .and_then(FormulaValue::as_number)
                .unwrap_or(0.0) as i32;
            let factor = 10_f64.powi(places);
            Ok(FormulaValue::Number((n * factor).round() / factor))
        }
        "floor" => {
            check_args(name, args, 1)?;
            Ok(FormulaValue::Number(require_number(name, &args[0])?.floor()))
        }
        "ceil" => {
            check_args(name, args, 1)?;
            Ok(FormulaValue::Number(require_number(name, &args[0])?.ceil()))
        }
        "sqrt" => {
            check_args(name, args, 1)?;
            Ok(FormulaValue::Number(require_number(name, &args[0])?.sqrt()))
        }
        "pow" => {
            check_args(name, args, 2)?;
            let base = require_number(name, &args[0])?;
            let exp = require_number(name, &args[1])?;
            Ok(FormulaValue::Number(base.powf(exp)))
        }
        "min" => {
            check_args(name, args, 2)?;
            let a = require_number(name, &args[0])?;
            let b = require_number(name, &args[1])?;
            Ok(FormulaValue::Number(a.min(b)))
        }
        "max" => {
            check_args(name, args, 2)?;
            let a = require_number(name, &args[0])?;
            let b = require_number(name, &args[1])?;
            Ok(FormulaValue::Number(a.max(b)))
        }

        // ── Date functions ──────────────────────────────────────────────
        "now" => {
            check_args(name, args, 0)?;
            let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
            Ok(FormulaValue::Date(now))
        }
        "year" => {
            check_args(name, args, 1)?;
            let date = require_date_str(name, &args[0])?;
            Ok(FormulaValue::Number(f64::from(date.year())))
        }
        "month" => {
            check_args(name, args, 1)?;
            let date = require_date_str(name, &args[0])?;
            Ok(FormulaValue::Number(f64::from(date.month())))
        }
        "day" => {
            check_args(name, args, 1)?;
            let date = require_date_str(name, &args[0])?;
            Ok(FormulaValue::Number(f64::from(date.day())))
        }
        #[allow(clippy::cast_possible_truncation)]
        "dateAdd" | "date_add" => {
            check_args(name, args, 3)?;
            let date = require_date_str(name, &args[0])?;
            let amount = require_number(name, &args[1])? as i64;
            let unit = args[2].to_display_string();
            let result = match unit.as_str() {
                "days" | "day" => date + chrono::Duration::days(amount),
                "weeks" | "week" => date + chrono::Duration::weeks(amount),
                "hours" | "hour" => date + chrono::Duration::hours(amount),
                _ => {
                    return Err(formula_err(
                        name,
                        &format!("unsupported date unit: '{unit}'"),
                    ))
                }
            };
            Ok(FormulaValue::Date(
                result.format("%Y-%m-%dT%H:%M:%S").to_string(),
            ))
        }
        #[allow(clippy::cast_precision_loss)]
        "dateBetween" | "date_between" => {
            check_args(name, args, 3)?;
            let d1 = require_date_str(name, &args[0])?;
            let d2 = require_date_str(name, &args[1])?;
            let unit = args[2].to_display_string();
            let diff = d1.signed_duration_since(d2);
            let result = match unit.as_str() {
                "days" | "day" => diff.num_days() as f64,
                "hours" | "hour" => diff.num_hours() as f64,
                "minutes" | "minute" => diff.num_minutes() as f64,
                _ => {
                    return Err(formula_err(
                        name,
                        &format!("unsupported date unit: '{unit}'"),
                    ))
                }
            };
            Ok(FormulaValue::Number(result))
        }
        "toDate" | "to_date" => {
            check_args(name, args, 1)?;
            let s = args[0].to_display_string();
            // Try parsing common date formats.
            if chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").is_ok()
                || chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S").is_ok()
            {
                Ok(FormulaValue::Date(s))
            } else {
                Err(formula_err(name, &format!("cannot parse '{s}' as date")))
            }
        }

        // ── Type conversion ─────────────────────────────────────────────
        "toNumber" | "to_number" => {
            check_args(name, args, 1)?;
            match args[0].as_number() {
                Some(n) => Ok(FormulaValue::Number(n)),
                None => Ok(FormulaValue::Null),
            }
        }
        "toString" | "to_string" => {
            check_args(name, args, 1)?;
            Ok(FormulaValue::String(args[0].to_display_string()))
        }
        "empty" => {
            check_args(name, args, 1)?;
            let is_empty = match &args[0] {
                FormulaValue::Null => true,
                FormulaValue::String(s) => s.is_empty(),
                _ => false,
            };
            Ok(FormulaValue::Boolean(is_empty))
        }

        // ── Logical (also available as operators) ───────────────────────
        "and" => {
            check_args(name, args, 2)?;
            Ok(FormulaValue::Boolean(
                args[0].is_truthy() && args[1].is_truthy(),
            ))
        }
        "or" => {
            check_args(name, args, 2)?;
            Ok(FormulaValue::Boolean(
                args[0].is_truthy() || args[1].is_truthy(),
            ))
        }
        "not" => {
            check_args(name, args, 1)?;
            Ok(FormulaValue::Boolean(!args[0].is_truthy()))
        }

        _ => Err(DatabaseError::FormulaError {
            position: 0,
            message: format!("unknown function: '{name}'"),
        }),
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn formula_err(func: &str, message: &str) -> DatabaseError {
    DatabaseError::FormulaError {
        position: 0,
        message: format!("{func}(): {message}"),
    }
}

fn check_args(name: &str, args: &[FormulaValue], expected: usize) -> Result<()> {
    if args.len() != expected {
        return Err(formula_err(
            name,
            &format!("expected {expected} argument(s), got {}", args.len()),
        ));
    }
    Ok(())
}

fn check_args_range(
    name: &str,
    args: &[FormulaValue],
    min: usize,
    max: usize,
) -> Result<()> {
    if args.len() < min || args.len() > max {
        return Err(formula_err(
            name,
            &format!(
                "expected {min}–{max} argument(s), got {}",
                args.len()
            ),
        ));
    }
    Ok(())
}

fn require_number(func: &str, val: &FormulaValue) -> Result<f64> {
    val.as_number()
        .ok_or_else(|| formula_err(func, "expected a number"))
}

fn require_date_str(
    func: &str,
    val: &FormulaValue,
) -> Result<chrono::NaiveDateTime> {
    let s = match val {
        FormulaValue::Date(d) => d.clone(),
        FormulaValue::String(s) => s.clone(),
        _ => {
            return Err(formula_err(func, "expected a date or date string"));
        }
    };

    // Try datetime first, then date-only (adding midnight).
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S") {
        return Ok(dt);
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
        return Ok(d.and_hms_opt(0, 0, 0).unwrap());
    }

    Err(formula_err(func, &format!("cannot parse '{s}' as date")))
}

use chrono::Datelike;

#[cfg(test)]
// 3.14 / 3.14159 in these tests are arbitrary non-zero doubles,
// not intended as approximations of PI — silence approx_constant.
#[allow(clippy::approx_constant)]
mod tests {
    use super::*;

    // Test helper — Vec callers match the ergonomics of `vec![...]`
    // literals at each test site.
    #[allow(clippy::needless_pass_by_value)]
    fn call_ok(name: &str, args: Vec<FormulaValue>) -> FormulaValue {
        call(name, &args).unwrap()
    }

    // ── String functions ────────────────────────────────────────────────

    #[test]
    fn fn_concat() {
        assert_eq!(
            call_ok(
                "concat",
                vec![
                    FormulaValue::String("a".to_string()),
                    FormulaValue::String("b".to_string()),
                    FormulaValue::String("c".to_string()),
                ]
            ),
            FormulaValue::String("abc".to_string())
        );
    }

    #[test]
    fn fn_upper() {
        assert_eq!(
            call_ok("upper", vec![FormulaValue::String("hello".to_string())]),
            FormulaValue::String("HELLO".to_string())
        );
    }

    #[test]
    fn fn_lower() {
        assert_eq!(
            call_ok("lower", vec![FormulaValue::String("HELLO".to_string())]),
            FormulaValue::String("hello".to_string())
        );
    }

    #[test]
    fn fn_trim() {
        assert_eq!(
            call_ok("trim", vec![FormulaValue::String("  hi  ".to_string())]),
            FormulaValue::String("hi".to_string())
        );
    }

    #[test]
    fn fn_len() {
        assert_eq!(
            call_ok("len", vec![FormulaValue::String("abc".to_string())]),
            FormulaValue::Number(3.0)
        );
    }

    #[test]
    fn fn_replace() {
        assert_eq!(
            call_ok(
                "replace",
                vec![
                    FormulaValue::String("hello world".to_string()),
                    FormulaValue::String("world".to_string()),
                    FormulaValue::String("rust".to_string()),
                ]
            ),
            FormulaValue::String("hello rust".to_string())
        );
    }

    #[test]
    fn fn_contains() {
        assert_eq!(
            call_ok(
                "contains",
                vec![
                    FormulaValue::String("hello world".to_string()),
                    FormulaValue::String("world".to_string()),
                ]
            ),
            FormulaValue::Boolean(true)
        );
    }

    // ── Numeric functions ───────────────────────────────────────────────

    #[test]
    fn fn_abs() {
        assert_eq!(
            call_ok("abs", vec![FormulaValue::Number(-5.0)]),
            FormulaValue::Number(5.0)
        );
    }

    #[test]
    fn fn_round() {
        assert_eq!(
            call_ok(
                "round",
                vec![FormulaValue::Number(3.14159), FormulaValue::Number(2.0)]
            ),
            FormulaValue::Number(3.14)
        );
    }

    #[test]
    fn fn_floor_ceil() {
        assert_eq!(
            call_ok("floor", vec![FormulaValue::Number(3.7)]),
            FormulaValue::Number(3.0)
        );
        assert_eq!(
            call_ok("ceil", vec![FormulaValue::Number(3.2)]),
            FormulaValue::Number(4.0)
        );
    }

    #[test]
    fn fn_sqrt() {
        assert_eq!(
            call_ok("sqrt", vec![FormulaValue::Number(9.0)]),
            FormulaValue::Number(3.0)
        );
    }

    #[test]
    fn fn_pow() {
        assert_eq!(
            call_ok(
                "pow",
                vec![FormulaValue::Number(2.0), FormulaValue::Number(3.0)]
            ),
            FormulaValue::Number(8.0)
        );
    }

    #[test]
    fn fn_min_max() {
        assert_eq!(
            call_ok(
                "min",
                vec![FormulaValue::Number(3.0), FormulaValue::Number(7.0)]
            ),
            FormulaValue::Number(3.0)
        );
        assert_eq!(
            call_ok(
                "max",
                vec![FormulaValue::Number(3.0), FormulaValue::Number(7.0)]
            ),
            FormulaValue::Number(7.0)
        );
    }

    // ── Date functions ──────────────────────────────────────────────────

    #[test]
    fn fn_year_month_day() {
        let date = FormulaValue::Date("2026-04-15T10:30:00".to_string());
        assert_eq!(call_ok("year", vec![date.clone()]), FormulaValue::Number(2026.0));
        assert_eq!(call_ok("month", vec![date.clone()]), FormulaValue::Number(4.0));
        assert_eq!(call_ok("day", vec![date]), FormulaValue::Number(15.0));
    }

    #[test]
    fn fn_date_add() {
        let result = call_ok(
            "dateAdd",
            vec![
                FormulaValue::Date("2026-04-15T00:00:00".to_string()),
                FormulaValue::Number(3.0),
                FormulaValue::String("days".to_string()),
            ],
        );
        assert!(matches!(result, FormulaValue::Date(s) if s.starts_with("2026-04-18")));
    }

    #[test]
    fn fn_date_between() {
        let result = call_ok(
            "dateBetween",
            vec![
                FormulaValue::Date("2026-04-18T00:00:00".to_string()),
                FormulaValue::Date("2026-04-15T00:00:00".to_string()),
                FormulaValue::String("days".to_string()),
            ],
        );
        assert_eq!(result, FormulaValue::Number(3.0));
    }

    // ── Type conversion ─────────────────────────────────────────────────

    #[test]
    fn fn_to_number() {
        assert_eq!(
            call_ok("toNumber", vec![FormulaValue::String("42".to_string())]),
            FormulaValue::Number(42.0)
        );
    }

    #[test]
    fn fn_to_string() {
        assert_eq!(
            call_ok("toString", vec![FormulaValue::Number(42.0)]),
            FormulaValue::String("42".to_string())
        );
    }

    #[test]
    fn fn_empty() {
        assert_eq!(
            call_ok("empty", vec![FormulaValue::Null]),
            FormulaValue::Boolean(true)
        );
        assert_eq!(
            call_ok("empty", vec![FormulaValue::String("hi".to_string())]),
            FormulaValue::Boolean(false)
        );
    }

    // ── Error cases ─────────────────────────────────────────────────────

    #[test]
    fn unknown_function() {
        assert!(call("nonexistent", &[]).is_err());
    }

    #[test]
    fn wrong_arg_count() {
        assert!(call("upper", &[]).is_err());
    }

    #[test]
    fn type_mismatch() {
        assert!(call("abs", &[FormulaValue::String("not a number".to_string())]).is_err());
    }
}
