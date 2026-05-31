//! Rich property type definitions for the database engine.
//!
//! [`PropertyConfig`] extends the flat `FieldDefinition` from `nexus-formats`
//! with typed configuration (select options with colors, number formats,
//! relation targets, rollup aggregation, formula expressions).
//!
//! [`PropertyValue`] is the engine's typed representation of a field value,
//! convertible to/from `serde_json::Value` for storage in `records.json`.

use serde::{Deserialize, Serialize};

// ── Property configuration ──────────────────────────────────────────────────

/// Rich configuration for a single property (field) in a database.
///
/// Stored as JSON inside the schema and deserialized for engine operations
/// (validation, formula evaluation, query compilation).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum PropertyConfig {
    /// Short text field.
    Text {
        /// Maximum character length (optional).
        #[serde(skip_serializing_if = "Option::is_none")]
        max_length: Option<usize>,
    },
    /// Long-form text / rich text.
    LongText {
        /// Maximum character length (optional).
        #[serde(skip_serializing_if = "Option::is_none")]
        max_length: Option<usize>,
    },
    /// Numeric value.
    Number {
        /// Display format.
        #[serde(default)]
        format: NumberFormat,
        /// Minimum allowed value.
        #[serde(skip_serializing_if = "Option::is_none")]
        min: Option<f64>,
        /// Maximum allowed value.
        #[serde(skip_serializing_if = "Option::is_none")]
        max: Option<f64>,
    },
    /// Currency value.
    Currency {
        /// Currency symbol (e.g. "$", "EUR").
        #[serde(default = "default_currency_symbol")]
        symbol: String,
        /// Decimal places.
        #[serde(default = "default_decimal_places")]
        decimal_places: u8,
    },
    /// Percentage value (stored as 0.0–1.0, displayed as 0–100%).
    Percent {
        /// Decimal places for display.
        #[serde(default)]
        decimal_places: u8,
    },
    /// Boolean checkbox.
    Checkbox,
    /// Date (no time component).
    Date {
        /// Display format.
        #[serde(default)]
        format: DateFormat,
    },
    /// Time only.
    Time,
    /// Date with time.
    Datetime {
        /// Display format.
        #[serde(default)]
        format: DateFormat,
    },
    /// Single-select from a predefined list.
    Select {
        /// Available options.
        #[serde(default)]
        options: Vec<SelectOption>,
    },
    /// Multi-select from a predefined list.
    MultiSelect {
        /// Available options.
        #[serde(default)]
        options: Vec<SelectOption>,
    },
    /// Relation to records in another database.
    Relation {
        /// Path or ID of the target database.
        target_database: String,
    },
    /// Rollup: aggregation over related records.
    Rollup {
        /// Name of the relation property to follow.
        relation_property: String,
        /// Property in the target database to aggregate.
        target_property: String,
        /// Aggregation function.
        aggregation: RollupAggregation,
    },
    /// Computed formula.
    Formula {
        /// The formula expression (e.g., `if(prop("status") == "done", 1, 0)`).
        expression: String,
    },
    /// Lookup: read-only view of a property through a relation.
    Lookup {
        /// Name of the relation property to follow.
        relation_property: String,
        /// Property in the target database to display.
        target_property: String,
    },
    /// Auto-generated UUID primary key.
    Uuid,
    /// URL field.
    Url,
    /// Email address field.
    Email,
    /// Phone number field.
    Phone,
}

fn default_currency_symbol() -> String {
    "$".to_string()
}

fn default_decimal_places() -> u8 {
    2
}

// ── Select options ──────────────────────────────────────────────────────────

/// A single option in a Select or `MultiSelect` property.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectOption {
    /// Stable identifier (survives renames).
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Color for UI rendering (e.g., "blue", "red", "green").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

// ── Enums ───────────────────────────────────────────────────────────────────

/// Display format for numeric values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NumberFormat {
    /// Plain number (e.g., 1234.56).
    #[default]
    Number,
    /// Number with thousands separator (e.g., 1,234.56).
    NumberWithCommas,
    /// Percent (e.g., 85%).
    Percent,
    /// US dollar (e.g., $1,234.56).
    Dollar,
    /// Euro (e.g., 1.234,56 EUR).
    Euro,
    /// British pound.
    Pound,
    /// Japanese yen.
    Yen,
}

/// Display format for date values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DateFormat {
    /// Full format: "April 15, 2026".
    #[default]
    Full,
    /// Relative: "3 days ago".
    Relative,
    /// ISO-ish: "2026-04-15".
    YearMonthDay,
    /// US style: "04/15/2026".
    MonthDayYear,
    /// European style: "15/04/2026".
    DayMonthYear,
}

/// Aggregation function for rollup properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RollupAggregation {
    /// Sum of numeric values.
    Sum,
    /// Arithmetic mean.
    Average,
    /// Minimum value.
    Min,
    /// Maximum value.
    Max,
    /// Count of all records.
    Count,
    /// Count of distinct values.
    CountUnique,
    /// Count of non-empty values.
    CountValues,
    /// Count of empty (null) values.
    CountEmpty,
    /// Count of non-empty values.
    CountNotEmpty,
    /// Percentage of empty values.
    PercentEmpty,
    /// Percentage of non-empty values.
    PercentNotEmpty,
}

// ── Property values ─────────────────────────────────────────────────────────

/// Typed representation of a property value in the database engine.
///
/// Converted to/from `serde_json::Value` for storage in `records.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropertyValue {
    /// No value.
    Null,
    /// Boolean value (checkbox).
    Boolean(bool),
    /// Numeric value.
    Number(f64),
    /// Text value (also used for select IDs, UUIDs, URLs, emails, phones).
    Text(String),
    /// Array of text values (multi-select, relation record IDs).
    TextArray(Vec<String>),
}

impl PropertyValue {
    /// Convert a `serde_json::Value` into a typed `PropertyValue` based on
    /// the property configuration.
    #[must_use]
    pub fn from_json(value: &serde_json::Value, _config: &PropertyConfig) -> Self {
        match value {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(b) => Self::Boolean(*b),
            serde_json::Value::Number(n) => Self::Number(n.as_f64().unwrap_or(0.0)),
            serde_json::Value::String(s) => Self::Text(s.clone()),
            serde_json::Value::Array(arr) => {
                let strings: Vec<String> = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                Self::TextArray(strings)
            }
            serde_json::Value::Object(_) => {
                // Objects are stored as JSON text for forward compatibility.
                Self::Text(value.to_string())
            }
        }
    }

    /// Convert this typed value back to a `serde_json::Value` for storage.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Self::Null => serde_json::Value::Null,
            Self::Boolean(b) => serde_json::Value::Bool(*b),
            Self::Number(n) => serde_json::json!(n),
            Self::Text(s) => serde_json::Value::String(s.clone()),
            Self::TextArray(arr) => serde_json::json!(arr),
        }
    }

    /// Coerce to a display string.
    #[must_use]
    pub fn as_display_string(&self) -> String {
        match self {
            Self::Null => String::new(),
            Self::Boolean(b) => if *b { "true" } else { "false" }.to_string(),
            Self::Number(n) => n.to_string(),
            Self::Text(s) => s.clone(),
            Self::TextArray(arr) => arr.join(", "),
        }
    }

    /// Try to extract as a number.
    #[must_use]
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            Self::Text(s) => s.parse().ok(),
            Self::Boolean(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    /// Check if the value is empty/null.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Null => true,
            Self::Text(s) => s.is_empty(),
            Self::TextArray(arr) => arr.is_empty(),
            _ => false,
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_config_select_roundtrip() {
        let config = PropertyConfig::Select {
            options: vec![
                SelectOption {
                    id: "opt_1".to_string(),
                    name: "To Do".to_string(),
                    color: Some("gray".to_string()),
                },
                SelectOption {
                    id: "opt_2".to_string(),
                    name: "Done".to_string(),
                    color: Some("green".to_string()),
                },
            ],
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: PropertyConfig = serde_json::from_str(&json).unwrap();
        match parsed {
            PropertyConfig::Select { options } => {
                assert_eq!(options.len(), 2);
                assert_eq!(options[0].name, "To Do");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn property_config_number_with_bounds() {
        let json = r#"{"type":"number","format":"number","min":0.0,"max":100.0}"#;
        let config: PropertyConfig = serde_json::from_str(json).unwrap();
        match config {
            PropertyConfig::Number { min, max, .. } => {
                assert_eq!(min, Some(0.0));
                assert_eq!(max, Some(100.0));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn property_config_formula() {
        let json = r#"{"type":"formula","expression":"prop(\"status\") == \"done\""}"#;
        let config: PropertyConfig = serde_json::from_str(json).unwrap();
        match config {
            PropertyConfig::Formula { expression } => {
                assert!(expression.contains("status"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn property_value_from_json_string() {
        let config = PropertyConfig::Text { max_length: None };
        let val = PropertyValue::from_json(&serde_json::json!("hello"), &config);
        assert_eq!(val, PropertyValue::Text("hello".to_string()));
    }

    #[test]
    fn property_value_from_json_number() {
        let config = PropertyConfig::Number {
            format: NumberFormat::Number,
            min: None,
            max: None,
        };
        let val = PropertyValue::from_json(&serde_json::json!(42.5), &config);
        assert_eq!(val, PropertyValue::Number(42.5));
    }

    #[test]
    fn property_value_from_json_array() {
        let config = PropertyConfig::MultiSelect { options: vec![] };
        let val = PropertyValue::from_json(&serde_json::json!(["a", "b"]), &config);
        assert_eq!(
            val,
            PropertyValue::TextArray(vec!["a".to_string(), "b".to_string()])
        );
    }

    #[test]
    // 3.14 is an arbitrary non-zero test value, not an approximation of PI.
    #[allow(clippy::approx_constant)]
    fn property_value_roundtrip() {
        let original = PropertyValue::Number(3.14);
        let json = original.to_json();
        let config = PropertyConfig::Number {
            format: NumberFormat::Number,
            min: None,
            max: None,
        };
        let restored = PropertyValue::from_json(&json, &config);
        assert_eq!(original, restored);
    }

    #[test]
    fn property_value_is_empty() {
        assert!(PropertyValue::Null.is_empty());
        assert!(PropertyValue::Text(String::new()).is_empty());
        assert!(PropertyValue::TextArray(vec![]).is_empty());
        assert!(!PropertyValue::Number(0.0).is_empty());
        assert!(!PropertyValue::Text("x".to_string()).is_empty());
    }

    #[test]
    fn rollup_aggregation_serde() {
        let agg = RollupAggregation::CountUnique;
        let json = serde_json::to_string(&agg).unwrap();
        assert_eq!(json, "\"count_unique\"");
        let parsed: RollupAggregation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, RollupAggregation::CountUnique);
    }

    #[test]
    fn number_format_default() {
        assert_eq!(NumberFormat::default(), NumberFormat::Number);
    }

    #[test]
    fn date_format_default() {
        assert_eq!(DateFormat::default(), DateFormat::Full);
    }

    #[test]
    fn property_value_as_number_coercion() {
        assert_eq!(PropertyValue::Number(5.0).as_number(), Some(5.0));
        assert_eq!(
            PropertyValue::Text("42".to_string()).as_number(),
            Some(42.0)
        );
        assert_eq!(PropertyValue::Boolean(true).as_number(), Some(1.0));
        assert_eq!(PropertyValue::Null.as_number(), None);
    }
}
