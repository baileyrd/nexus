//! Shared types for `.bases` databases.
//!
//! These types describe a loaded base (schema, records, views, relations,
//! metadata) and a summary row for listings. They are shared between the
//! storage engine (which owns disk + `SQLite` I/O) and higher-level consumers
//! (database engine, CLI) so neither side needs to depend on the other for
//! type definitions.

use serde::{Deserialize, Serialize};

/// A complete base (database) loaded from a `.bases` directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Base {
    /// Human-readable database name.
    pub name: String,
    /// Field schema.
    pub schema: BaseSchema,
    /// Data records.
    pub records: Vec<BaseRecord>,
    /// View definitions.
    #[serde(default)]
    pub views: Vec<BaseView>,
    /// Relation definitions.
    #[serde(default)]
    pub relations: Vec<BaseRelation>,
    /// File metadata.
    #[serde(default)]
    pub metadata: BaseMetadata,
}

/// Schema defining the fields of a base.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseSchema {
    /// Schema format version.
    #[serde(default = "default_version")]
    pub version: String,
    /// Field definitions keyed by field name.
    pub fields: serde_json::Map<String, serde_json::Value>,
}

fn default_version() -> String {
    "1.0".to_string()
}

/// A single data record in a base.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseRecord {
    /// Record unique identifier.
    pub id: String,
    /// Field values (keys match schema field names).
    #[serde(flatten)]
    pub fields: serde_json::Map<String, serde_json::Value>,
}

/// A view definition for displaying base records.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseView {
    /// View display name.
    pub name: String,
    /// View type.
    #[serde(rename = "type")]
    pub view_type: ViewType,
    /// Visible field names.
    #[serde(default)]
    pub fields: Vec<String>,
    /// Sort rules.
    #[serde(default)]
    pub sort: Vec<SortRule>,
    /// Filter rules.
    #[serde(default)]
    pub filter: Vec<FilterRule>,
    /// Field to group by (for kanban views).
    #[serde(rename = "groupField", skip_serializing_if = "Option::is_none")]
    pub group_field: Option<String>,
    /// Date field (for calendar views).
    #[serde(rename = "dateField", skip_serializing_if = "Option::is_none")]
    pub date_field: Option<String>,
}

/// View display type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ViewType {
    /// Spreadsheet-style table.
    Table,
    /// Kanban board grouped by a field.
    Kanban,
    /// Calendar view by date field.
    Calendar,
    /// Gallery card view.
    Gallery,
}

/// Sort rule for a view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortRule {
    /// Field name to sort by.
    pub field: String,
    /// Sort direction.
    #[serde(default = "default_sort_dir")]
    pub direction: String,
}

fn default_sort_dir() -> String {
    "asc".to_string()
}

/// Filter rule for a view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterRule {
    /// Field name to filter on.
    pub field: String,
    /// Operator (eq, neq, gt, lt, contains, etc.).
    pub operator: String,
    /// Value to compare against.
    pub value: serde_json::Value,
}

/// Relation between two bases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseRelation {
    /// Relation name.
    pub name: String,
    /// Relation type.
    #[serde(rename = "type")]
    pub relation_type: String,
    /// Source field name.
    #[serde(rename = "sourceField")]
    pub source_field: String,
    /// Target base path.
    #[serde(rename = "targetBase")]
    pub target_base: String,
    /// Target field name.
    #[serde(rename = "targetField")]
    pub target_field: String,
}

/// Base metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BaseMetadata {
    /// Format version.
    pub version: String,
    /// Creation timestamp (Unix seconds).
    pub created_at: i64,
    /// Last modification timestamp (Unix seconds).
    pub modified_at: i64,
}

impl Default for BaseMetadata {
    fn default() -> Self {
        Self {
            version: "1.0".to_string(),
            created_at: 0,
            modified_at: 0,
        }
    }
}

/// Summary of a base for listing.
#[derive(Debug, Clone)]
pub struct BaseSummary {
    /// Database row ID.
    pub id: i64,
    /// Vault-relative path to the .bases directory.
    pub path: String,
    /// Human-readable name.
    pub name: String,
    /// Number of records.
    pub record_count: i64,
}
