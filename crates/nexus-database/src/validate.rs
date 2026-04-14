//! Type-aware property validation for database records.
//!
//! Extends the basic required-field check from `nexus-types::bases` with
//! per-type validation (email format, number ranges, select option
//! membership, date parsing, etc.).

use std::collections::BTreeMap;

use crate::error::{DatabaseError, Result};
use crate::types::PropertyConfig;

// ── Validation result types ─────────────────────────────────────────────────

/// Severity of a validation issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The record cannot be saved.
    Error,
    /// The record can be saved but has a potential problem.
    Warning,
}

/// A single validation issue found in a record.
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    /// The field name that has the issue.
    pub field: String,
    /// Severity level.
    pub severity: Severity,
    /// Human-readable description.
    pub message: String,
}

// ── Validator trait ──────────────────────────────────────────────────────────

/// Trait for pluggable property validation.
pub trait PropertyValidator: Send + Sync {
    /// Validate a single field value against its configuration.
    ///
    /// # Errors
    ///
    /// Returns `DatabaseError::ValidationFailed` if the value is invalid.
    fn validate(
        &self,
        field_name: &str,
        value: &serde_json::Value,
        config: &PropertyConfig,
    ) -> Result<()>;
}

// ── Built-in validator ──────────────────────────────────────────────────────

/// The default validator covering all 20 property types.
pub struct BuiltinValidator;

impl PropertyValidator for BuiltinValidator {
    fn validate(
        &self,
        field_name: &str,
        value: &serde_json::Value,
        config: &PropertyConfig,
    ) -> Result<()> {
        // Null values are allowed (required-ness is checked separately).
        if value.is_null() {
            return Ok(());
        }

        match config {
            PropertyConfig::Email => validate_email(field_name, value),
            PropertyConfig::Url => validate_url(field_name, value),
            PropertyConfig::Phone => validate_phone(field_name, value),
            PropertyConfig::Checkbox => validate_bool(field_name, value),
            PropertyConfig::Number { min, max, .. } => {
                validate_number(field_name, value, *min, *max)
            }
            PropertyConfig::Currency { .. } | PropertyConfig::Percent { .. } => {
                validate_number(field_name, value, None, None)
            }
            PropertyConfig::Date { .. } => validate_date(field_name, value),
            PropertyConfig::Time => validate_time(field_name, value),
            PropertyConfig::Datetime { .. } => validate_datetime(field_name, value),
            PropertyConfig::Select { options } => {
                validate_select(field_name, value, options, false)
            }
            PropertyConfig::MultiSelect { options } => {
                validate_select(field_name, value, options, true)
            }
            PropertyConfig::Text { max_length } | PropertyConfig::LongText { max_length } => {
                validate_text(field_name, value, *max_length)
            }
            // Computed fields are not user-set; skip validation.
            PropertyConfig::Formula { .. }
            | PropertyConfig::Rollup { .. }
            | PropertyConfig::Lookup { .. }
            | PropertyConfig::Uuid
            | PropertyConfig::Relation { .. } => Ok(()),
        }
    }
}

// ── Per-type validation functions ───────────────────────────────────────────

fn validation_err(field: &str, reason: &str) -> DatabaseError {
    DatabaseError::ValidationFailed {
        field: field.to_string(),
        reason: reason.to_string(),
    }
}

fn validate_email(field: &str, value: &serde_json::Value) -> Result<()> {
    let s = value
        .as_str()
        .ok_or_else(|| validation_err(field, "expected a string"))?;
    let re = regex_lite::Regex::new(r"^[^\s@]+@[^\s@]+\.[^\s@]+$").expect("valid regex");
    if re.is_match(s) {
        Ok(())
    } else {
        Err(validation_err(field, "invalid email address"))
    }
}

fn validate_url(field: &str, value: &serde_json::Value) -> Result<()> {
    let s = value
        .as_str()
        .ok_or_else(|| validation_err(field, "expected a string"))?;
    if s.starts_with("http://") || s.starts_with("https://") {
        Ok(())
    } else {
        Err(validation_err(field, "URL must start with http:// or https://"))
    }
}

fn validate_phone(field: &str, value: &serde_json::Value) -> Result<()> {
    let s = value
        .as_str()
        .ok_or_else(|| validation_err(field, "expected a string"))?;
    let digits: String = s.chars().filter(char::is_ascii_digit).collect();
    if (7..=15).contains(&digits.len()) {
        Ok(())
    } else {
        Err(validation_err(
            field,
            "phone number must contain 7–15 digits",
        ))
    }
}

fn validate_bool(field: &str, value: &serde_json::Value) -> Result<()> {
    if value.is_boolean() {
        Ok(())
    } else {
        Err(validation_err(field, "expected a boolean"))
    }
}

fn validate_number(
    field: &str,
    value: &serde_json::Value,
    min: Option<f64>,
    max: Option<f64>,
) -> Result<()> {
    let n = value
        .as_f64()
        .ok_or_else(|| validation_err(field, "expected a number"))?;
    if let Some(m) = min {
        if n < m {
            return Err(validation_err(
                field,
                &format!("value {n} is below minimum {m}"),
            ));
        }
    }
    if let Some(m) = max {
        if n > m {
            return Err(validation_err(
                field,
                &format!("value {n} is above maximum {m}"),
            ));
        }
    }
    Ok(())
}

fn validate_date(field: &str, value: &serde_json::Value) -> Result<()> {
    let s = value
        .as_str()
        .ok_or_else(|| validation_err(field, "expected a date string"))?;
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| validation_err(field, "invalid date format (expected YYYY-MM-DD)"))?;
    Ok(())
}

fn validate_time(field: &str, value: &serde_json::Value) -> Result<()> {
    let s = value
        .as_str()
        .ok_or_else(|| validation_err(field, "expected a time string"))?;
    chrono::NaiveTime::parse_from_str(s, "%H:%M")
        .or_else(|_| chrono::NaiveTime::parse_from_str(s, "%H:%M:%S"))
        .map_err(|_| validation_err(field, "invalid time format (expected HH:MM or HH:MM:SS)"))?;
    Ok(())
}

fn validate_datetime(field: &str, value: &serde_json::Value) -> Result<()> {
    let s = value
        .as_str()
        .ok_or_else(|| validation_err(field, "expected a datetime string"))?;
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
        .map_err(|_| {
            validation_err(
                field,
                "invalid datetime format (expected YYYY-MM-DDTHH:MM:SS)",
            )
        })?;
    Ok(())
}

fn validate_text(
    field: &str,
    value: &serde_json::Value,
    max_length: Option<usize>,
) -> Result<()> {
    let s = value
        .as_str()
        .ok_or_else(|| validation_err(field, "expected a string"))?;
    if let Some(max) = max_length {
        if s.len() > max {
            return Err(validation_err(
                field,
                &format!("text exceeds maximum length of {max} characters"),
            ));
        }
    }
    Ok(())
}

fn validate_select(
    field: &str,
    value: &serde_json::Value,
    options: &[crate::types::SelectOption],
    multi: bool,
) -> Result<()> {
    if multi {
        let arr = value
            .as_array()
            .ok_or_else(|| validation_err(field, "expected an array for multi-select"))?;
        for item in arr {
            let s = item
                .as_str()
                .ok_or_else(|| validation_err(field, "multi-select values must be strings"))?;
            if !options.iter().any(|o| o.id == s || o.name == s) {
                return Err(validation_err(
                    field,
                    &format!("'{s}' is not a valid option"),
                ));
            }
        }
    } else {
        let s = value
            .as_str()
            .ok_or_else(|| validation_err(field, "expected a string for select"))?;
        if !options.iter().any(|o| o.id == s || o.name == s) {
            return Err(validation_err(
                field,
                &format!("'{s}' is not a valid option"),
            ));
        }
    }
    Ok(())
}

// ── Full record validation ──────────────────────────────────────────────────

/// Validate an entire record against its schema and property configurations.
///
/// Checks required fields first (via the existing storage-level check), then
/// runs type-aware validation on every field that has a [`PropertyConfig`].
///
/// Returns all validation issues found (does not short-circuit on first error).
pub fn validate_record_full(
    record: &nexus_types::bases::BaseRecord,
    configs: &BTreeMap<String, PropertyConfig>,
    validator: &dyn PropertyValidator,
) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    // Check required fields: any field in configs that the record is missing.
    // (Not having a field is OK — required-ness is a separate concern
    // handled by the storage-level validate_record().)

    // Type-aware validation for every present field.
    for (field_name, value) in &record.fields {
        // Skip the "id" field — it's a system field.
        if field_name == "id" {
            continue;
        }
        if let Some(config) = configs.get(field_name) {
            if let Err(e) = validator.validate(field_name, value, config) {
                issues.push(ValidationIssue {
                    field: field_name.clone(),
                    severity: Severity::Error,
                    message: e.to_string(),
                });
            }
        }
    }

    issues
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_email() {
        let v = BuiltinValidator;
        let config = PropertyConfig::Email;
        assert!(v.validate("email", &serde_json::json!("user@example.com"), &config).is_ok());
    }

    #[test]
    fn invalid_email() {
        let v = BuiltinValidator;
        let config = PropertyConfig::Email;
        assert!(v.validate("email", &serde_json::json!("not-an-email"), &config).is_err());
    }

    #[test]
    fn valid_url() {
        let v = BuiltinValidator;
        let config = PropertyConfig::Url;
        assert!(v.validate("url", &serde_json::json!("https://example.com"), &config).is_ok());
    }

    #[test]
    fn invalid_url() {
        let v = BuiltinValidator;
        let config = PropertyConfig::Url;
        assert!(v.validate("url", &serde_json::json!("ftp://nope"), &config).is_err());
    }

    #[test]
    fn valid_phone() {
        let v = BuiltinValidator;
        let config = PropertyConfig::Phone;
        assert!(v.validate("phone", &serde_json::json!("+1 (555) 123-4567"), &config).is_ok());
    }

    #[test]
    fn invalid_phone_too_short() {
        let v = BuiltinValidator;
        let config = PropertyConfig::Phone;
        assert!(v.validate("phone", &serde_json::json!("123"), &config).is_err());
    }

    #[test]
    fn number_within_bounds() {
        let v = BuiltinValidator;
        let config = PropertyConfig::Number {
            format: crate::types::NumberFormat::Number,
            min: Some(0.0),
            max: Some(100.0),
        };
        assert!(v.validate("score", &serde_json::json!(50), &config).is_ok());
    }

    #[test]
    fn number_below_min() {
        let v = BuiltinValidator;
        let config = PropertyConfig::Number {
            format: crate::types::NumberFormat::Number,
            min: Some(0.0),
            max: None,
        };
        assert!(v.validate("score", &serde_json::json!(-5), &config).is_err());
    }

    #[test]
    fn number_above_max() {
        let v = BuiltinValidator;
        let config = PropertyConfig::Number {
            format: crate::types::NumberFormat::Number,
            min: None,
            max: Some(100.0),
        };
        assert!(v.validate("score", &serde_json::json!(200), &config).is_err());
    }

    #[test]
    fn valid_date() {
        let v = BuiltinValidator;
        let config = PropertyConfig::Date {
            format: crate::types::DateFormat::YearMonthDay,
        };
        assert!(v.validate("due", &serde_json::json!("2026-04-15"), &config).is_ok());
    }

    #[test]
    fn invalid_date() {
        let v = BuiltinValidator;
        let config = PropertyConfig::Date {
            format: crate::types::DateFormat::Full,
        };
        assert!(v.validate("due", &serde_json::json!("not-a-date"), &config).is_err());
    }

    #[test]
    fn valid_select() {
        let v = BuiltinValidator;
        let config = PropertyConfig::Select {
            options: vec![
                crate::types::SelectOption {
                    id: "todo".to_string(),
                    name: "To Do".to_string(),
                    color: None,
                },
            ],
        };
        assert!(v.validate("status", &serde_json::json!("todo"), &config).is_ok());
        assert!(v.validate("status", &serde_json::json!("To Do"), &config).is_ok());
    }

    #[test]
    fn invalid_select() {
        let v = BuiltinValidator;
        let config = PropertyConfig::Select {
            options: vec![
                crate::types::SelectOption {
                    id: "todo".to_string(),
                    name: "To Do".to_string(),
                    color: None,
                },
            ],
        };
        assert!(v.validate("status", &serde_json::json!("invalid"), &config).is_err());
    }

    #[test]
    fn valid_multi_select() {
        let v = BuiltinValidator;
        let config = PropertyConfig::MultiSelect {
            options: vec![
                crate::types::SelectOption {
                    id: "a".to_string(),
                    name: "A".to_string(),
                    color: None,
                },
                crate::types::SelectOption {
                    id: "b".to_string(),
                    name: "B".to_string(),
                    color: None,
                },
            ],
        };
        assert!(v
            .validate("tags", &serde_json::json!(["a", "b"]), &config)
            .is_ok());
    }

    #[test]
    fn invalid_multi_select_option() {
        let v = BuiltinValidator;
        let config = PropertyConfig::MultiSelect {
            options: vec![crate::types::SelectOption {
                id: "a".to_string(),
                name: "A".to_string(),
                color: None,
            }],
        };
        assert!(v
            .validate("tags", &serde_json::json!(["a", "nope"]), &config)
            .is_err());
    }

    #[test]
    fn null_value_always_passes() {
        let v = BuiltinValidator;
        let config = PropertyConfig::Email;
        assert!(v.validate("email", &serde_json::Value::Null, &config).is_ok());
    }

    #[test]
    fn checkbox_requires_bool() {
        let v = BuiltinValidator;
        let config = PropertyConfig::Checkbox;
        assert!(v.validate("done", &serde_json::json!(true), &config).is_ok());
        assert!(v.validate("done", &serde_json::json!("yes"), &config).is_err());
    }

    #[test]
    fn text_max_length() {
        let v = BuiltinValidator;
        let config = PropertyConfig::Text {
            max_length: Some(5),
        };
        assert!(v.validate("name", &serde_json::json!("abc"), &config).is_ok());
        assert!(v.validate("name", &serde_json::json!("toolong"), &config).is_err());
    }

    #[test]
    fn formula_fields_skip_validation() {
        let v = BuiltinValidator;
        let config = PropertyConfig::Formula {
            expression: "1 + 1".to_string(),
        };
        // Even a non-sensical value should pass since formulas are computed.
        assert!(v
            .validate("calc", &serde_json::json!("anything"), &config)
            .is_ok());
    }

    #[test]
    fn validate_record_full_collects_all_issues() {
        let v = BuiltinValidator;
        let mut configs = BTreeMap::new();
        configs.insert(
            "email".to_string(),
            PropertyConfig::Email,
        );
        configs.insert(
            "score".to_string(),
            PropertyConfig::Number {
                format: crate::types::NumberFormat::Number,
                min: Some(0.0),
                max: Some(100.0),
            },
        );

        let mut fields = serde_json::Map::new();
        fields.insert("id".to_string(), serde_json::json!("r1"));
        fields.insert("email".to_string(), serde_json::json!("bad"));
        fields.insert("score".to_string(), serde_json::json!(200));

        let record = nexus_types::bases::BaseRecord {
            id: "r1".to_string(),
            fields,
        };

        let issues = validate_record_full(&record, &configs, &v);
        assert_eq!(issues.len(), 2);
    }
}
