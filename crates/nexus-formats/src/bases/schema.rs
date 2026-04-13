//! `.bases` schema definitions (`schema.json`).

use serde::{Deserialize, Serialize};

/// Schema defining the fields of a base database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseSchema {
    /// Schema format version (e.g. `"1.0"`).
    #[serde(default = "default_version")]
    pub version: String,
    /// Field definitions keyed by field name.
    pub fields: std::collections::BTreeMap<String, FieldDefinition>,
}

fn default_version() -> String {
    "1.0".to_string()
}

impl Default for BaseSchema {
    fn default() -> Self {
        Self {
            version: "1.0".to_string(),
            fields: std::collections::BTreeMap::new(),
        }
    }
}

/// A single field definition in a base schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDefinition {
    /// Field data type.
    #[serde(rename = "type")]
    pub field_type: FieldType,
    /// Whether the field must have a value on every record.
    #[serde(default)]
    pub required: bool,
    /// Whether this is the primary key field.
    #[serde(default)]
    pub primary: bool,
    /// Allowed values for `select` / `multi-select` fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
    /// Minimum value for numeric fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    /// Maximum value for numeric fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Target `.bases` path for relation fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Target field name in the related base.
    #[serde(rename = "targetField", skip_serializing_if = "Option::is_none")]
    pub target_field: Option<String>,
}

/// All supported field data types in a base schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FieldType {
    /// Short text.
    Text,
    /// Long-form text (multi-line).
    LongText,
    /// Numeric value.
    Number,
    /// Currency value.
    Currency,
    /// Percentage value.
    Percent,
    /// Boolean checkbox.
    Checkbox,
    /// Date value.
    Date,
    /// Time value.
    Time,
    /// Combined date and time.
    Datetime,
    /// Single-select from an options list.
    Select,
    /// Multi-select from an options list.
    MultiSelect,
    /// Relation to another base.
    Relation,
    /// Computed formula.
    Formula,
    /// Rollup across related records.
    Rollup,
    /// Lookup into related records.
    Lookup,
    /// UUID primary key.
    Uuid,
    /// URL.
    Url,
    /// Email address.
    Email,
    /// Phone number.
    Phone,
}
