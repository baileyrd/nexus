//! `.bases` view, relation, and metadata definitions (`views.toml`, `relations.toml`, `metadata.json`).

use serde::{Deserialize, Serialize};

/// A view definition from `views.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BaseView {
    /// Human-readable view name.
    pub name: String,
    /// View type.
    #[serde(rename = "type")]
    pub view_type: ViewType,
    /// Ordered list of fields to display.
    #[serde(default)]
    pub fields: Vec<String>,
    /// Sort rules.
    #[serde(default)]
    pub sort: Vec<SortRule>,
    /// Filter expressions.
    #[serde(default)]
    pub filter: Vec<String>,
    /// Group-by field for kanban views.
    #[serde(rename = "groupField", skip_serializing_if = "Option::is_none")]
    pub group_field: Option<String>,
    /// Date field for calendar views.
    #[serde(rename = "dateField", skip_serializing_if = "Option::is_none")]
    pub date_field: Option<String>,
}

/// Supported view layout types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ViewType {
    /// Tabular grid view.
    #[default]
    Table,
    /// Kanban board grouped by a field.
    Kanban,
    /// Calendar view by date field.
    Calendar,
    /// Card gallery.
    Gallery,
    /// Timeline / Gantt view.
    Timeline,
}

/// A sort rule within a view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortRule {
    /// Field to sort by.
    pub field: String,
    /// Sort direction.
    pub direction: SortDirection,
}

/// Sort direction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortDirection {
    /// Ascending (A→Z, 0→9).
    Asc,
    /// Descending (Z→A, 9→0).
    Desc,
}

/// A relation definition from `relations.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseRelation {
    /// Relation type.
    #[serde(rename = "type")]
    pub relation_type: RelationType,
    /// Source field name in this base.
    #[serde(rename = "sourceField")]
    pub source_field: String,
    /// Target `.bases` path.
    #[serde(rename = "targetBase")]
    pub target_base: String,
    /// Target field in the related base.
    #[serde(rename = "targetField")]
    pub target_field: String,
}

/// Relation cardinality type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RelationType {
    /// One record in this base → many in the target.
    OneToMany,
    /// Many records in this base → one in the target.
    ManyToOne,
    /// Many ↔ many.
    ManyToMany,
}

/// Metadata stored in `metadata.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseMetadata {
    /// Format version.
    #[serde(default = "default_version")]
    pub version: String,
    /// ISO-8601 creation timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    /// ISO-8601 last-modified timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    /// Human-readable database name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

fn default_version() -> String {
    "1.0".to_string()
}

impl Default for BaseMetadata {
    fn default() -> Self {
        Self {
            version:  "1.0".to_string(),
            created:  None,
            modified: None,
            name:     None,
        }
    }
}

/// Container for all view definitions parsed from `views.toml`.
///
/// `views.toml` uses a `[views.<id>]` table structure.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ViewsFile {
    /// Views keyed by their identifier.
    #[serde(default)]
    pub views: std::collections::BTreeMap<String, BaseView>,
}

/// Container for all relation definitions parsed from `relations.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RelationsFile {
    /// Relations keyed by their identifier.
    #[serde(default)]
    pub relations: std::collections::BTreeMap<String, BaseRelation>,
}
