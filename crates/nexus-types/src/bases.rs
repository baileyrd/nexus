//! Shared types and filesystem layer for `.bases` databases.
//!
//! Describes a loaded base (schema, records, views, relations, metadata) plus
//! a summary row for listings, and provides the pure-filesystem load/save/init
//! helpers. These live here rather than in `nexus-storage` so non-storage
//! consumers (database engine, CLI) can work with `.bases` directories
//! without pulling in a `SQLite` dep.

use std::path::Path;

use serde::{Deserialize, Serialize};

// ── Types ────────────────────────────────────────────────────────────────────

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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BaseRecord {
    /// Record unique identifier.
    pub id: String,
    /// Soft-delete timestamp (Unix epoch seconds). `None` = live
    /// record. Set by `base_record_soft_delete`; cleared by
    /// `base_record_restore`. Kept separate from
    /// `base_record_delete` which hard-removes from disk.
    #[serde(rename = "deletedAt", skip_serializing_if = "Option::is_none", default)]
    pub deleted_at: Option<i64>,
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
    /// Date field (for calendar views; also the start-date field for
    /// timeline views).
    #[serde(rename = "dateField", skip_serializing_if = "Option::is_none")]
    pub date_field: Option<String>,
    /// End-date field (timeline views only — pairs with `date_field`
    /// as the start). Absent for every other view type.
    #[serde(rename = "endField", skip_serializing_if = "Option::is_none")]
    pub end_field: Option<String>,
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
    /// List view grouped by a field.
    List,
    /// Timeline / gantt view — swimlanes by `group_field`, bars
    /// spanning `date_field` (start) → `end_field` (end).
    Timeline,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// A single field definition within a schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDefinition {
    /// Field data type.
    #[serde(rename = "type")]
    pub field_type: FieldType,
    /// Whether the field is required.
    #[serde(default)]
    pub required: bool,
    /// Whether this is the primary key.
    #[serde(default)]
    pub primary: bool,
    /// Allowed values for select/multi-select fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
    /// Minimum value for number fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    /// Maximum value for number fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Target base for relation fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Target field for relation fields.
    #[serde(rename = "targetField", skip_serializing_if = "Option::is_none")]
    pub target_field: Option<String>,
}

/// Supported field data types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FieldType {
    /// Short text.
    Text,
    /// Long-form text.
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
    /// Single-select from options list.
    Select,
    /// Multi-select from options list.
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
    /// URL field.
    Url,
    /// Email field.
    Email,
}

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors from bases filesystem operations.
#[derive(Debug, thiserror::Error)]
pub enum BasesError {
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A required file inside the `.bases` directory was missing.
    #[error("file not found: {0}")]
    FileNotFound(String),

    /// A bases file could not be parsed or serialised.
    #[error("corrupt bases file {path}: {reason}")]
    CorruptFile {
        /// Offending file path.
        path: String,
        /// Underlying parse/serialise error.
        reason: String,
    },

    /// A record failed schema validation.
    #[error("record validation failed: {0}")]
    ValidationFailed(String),
}

// ── Directory Load / Save ────────────────────────────────────────────────────

/// Load a complete base from a `.bases` directory.
///
/// Reads `schema.json`, `records.json`, `views.toml`, `relations.toml`,
/// and `metadata.json`. Missing optional files are treated as empty.
///
/// # Errors
///
/// Returns [`BasesError::FileNotFound`] if `schema.json` is missing,
/// [`BasesError::CorruptFile`] on parse failure, or [`BasesError::Io`]
/// on I/O failure.
pub fn load_base(dir: &Path) -> Result<Base, BasesError> {
    let name = dir
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled")
        .to_string();

    // schema.json — required
    let schema_path = dir.join("schema.json");
    let schema: BaseSchema = if schema_path.exists() {
        let text = std::fs::read_to_string(&schema_path)?;
        serde_json::from_str(&text).map_err(|e| BasesError::CorruptFile {
            path: schema_path.display().to_string(),
            reason: e.to_string(),
        })?
    } else {
        return Err(BasesError::FileNotFound(
            schema_path.display().to_string(),
        ));
    };

    // records.json — optional
    let records_path = dir.join("records.json");
    let records: Vec<BaseRecord> = if records_path.exists() {
        let text = std::fs::read_to_string(&records_path)?;
        serde_json::from_str(&text).map_err(|e| BasesError::CorruptFile {
            path: records_path.display().to_string(),
            reason: e.to_string(),
        })?
    } else {
        Vec::new()
    };

    // views.toml — optional
    let views_path = dir.join("views.toml");
    let views: Vec<BaseView> = if views_path.exists() {
        let text = std::fs::read_to_string(&views_path)?;
        let table: toml::Table = toml::from_str(&text).map_err(|e| BasesError::CorruptFile {
            path: views_path.display().to_string(),
            reason: e.to_string(),
        })?;
        parse_views_table(&table)?
    } else {
        Vec::new()
    };

    // relations.toml — optional
    let relations_path = dir.join("relations.toml");
    let relations: Vec<BaseRelation> = if relations_path.exists() {
        let text = std::fs::read_to_string(&relations_path)?;
        let table: toml::Table = toml::from_str(&text).map_err(|e| BasesError::CorruptFile {
            path: relations_path.display().to_string(),
            reason: e.to_string(),
        })?;
        parse_relations_table(&table)?
    } else {
        Vec::new()
    };

    // metadata.json — optional
    let meta_path = dir.join("metadata.json");
    let metadata: BaseMetadata = if meta_path.exists() {
        let text = std::fs::read_to_string(&meta_path)?;
        serde_json::from_str(&text).map_err(|e| BasesError::CorruptFile {
            path: meta_path.display().to_string(),
            reason: e.to_string(),
        })?
    } else {
        BaseMetadata::default()
    };

    Ok(Base {
        name,
        schema,
        records,
        views,
        relations,
        metadata,
    })
}

/// Save a complete base to a `.bases` directory.
///
/// Creates the directory if it doesn't exist. Writes all constituent files.
///
/// # Errors
///
/// Returns [`BasesError`] on I/O or serialization failure.
pub fn save_base(dir: &Path, base: &Base) -> Result<(), BasesError> {
    std::fs::create_dir_all(dir)?;

    // schema.json
    let schema_json =
        serde_json::to_string_pretty(&base.schema).map_err(|e| BasesError::CorruptFile {
            path: dir.join("schema.json").display().to_string(),
            reason: e.to_string(),
        })?;
    std::fs::write(dir.join("schema.json"), schema_json)?;

    // records.json
    let records_json =
        serde_json::to_string_pretty(&base.records).map_err(|e| BasesError::CorruptFile {
            path: dir.join("records.json").display().to_string(),
            reason: e.to_string(),
        })?;
    std::fs::write(dir.join("records.json"), records_json)?;

    // views.toml — remove when the list is empty so "delete the last view"
    // round-trips correctly. Leaving a stale file behind would reload as
    // views we just dropped.
    let views_path = dir.join("views.toml");
    if base.views.is_empty() {
        if views_path.exists() {
            std::fs::remove_file(&views_path)?;
        }
    } else {
        let views_toml = serialize_views_toml(&base.views)?;
        std::fs::write(&views_path, views_toml)?;
    }

    // relations.toml — same empty-state rule as views.toml.
    let relations_path = dir.join("relations.toml");
    if base.relations.is_empty() {
        if relations_path.exists() {
            std::fs::remove_file(&relations_path)?;
        }
    } else {
        let relations_toml = serialize_relations_toml(&base.relations)?;
        std::fs::write(&relations_path, relations_toml)?;
    }

    // metadata.json
    let meta_json =
        serde_json::to_string_pretty(&base.metadata).map_err(|e| BasesError::CorruptFile {
            path: dir.join("metadata.json").display().to_string(),
            reason: e.to_string(),
        })?;
    std::fs::write(dir.join("metadata.json"), meta_json)?;

    Ok(())
}

/// Initialise a new empty base directory with the given schema.
///
/// # Errors
///
/// Returns [`BasesError`] on I/O failure.
pub fn init_base(dir: &Path, name: &str, schema: &BaseSchema) -> Result<Base, BasesError> {
    let now = unix_now();
    let base = Base {
        name: name.to_string(),
        schema: schema.clone(),
        records: Vec::new(),
        views: Vec::new(),
        relations: Vec::new(),
        metadata: BaseMetadata {
            version: "1.0".to_string(),
            created_at: now,
            modified_at: now,
        },
    };
    save_base(dir, &base)?;
    Ok(base)
}

// ── Validation ───────────────────────────────────────────────────────────────

/// Validate a record against a schema.
///
/// **Scope is intentionally narrow**: this checks only that fields
/// declared with `required = true` in the schema are present on the
/// record. It does **not** verify field types, `min` / `max` numeric
/// bounds, `options` enum membership, or any other constraint the
/// schema may declare — those checks haven't been wired up yet (see
/// issue #82). The function name is broader than what it currently
/// guarantees; callers that need full schema validation should
/// not assume `Ok(())` means anything beyond "required-field
/// presence."
///
/// # Errors
///
/// Returns [`BasesError::ValidationFailed`] if a required field is missing.
pub fn validate_record(schema: &BaseSchema, record: &BaseRecord) -> Result<(), BasesError> {
    for (field_name, field_def) in &schema.fields {
        let required = field_def
            .get("required")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if required && !record.fields.contains_key(field_name) && field_name != "id" {
            return Err(BasesError::ValidationFailed(format!(
                "record {}: missing required field '{field_name}'",
                record.id
            )));
        }
    }
    Ok(())
}

// ── TOML Helpers ─────────────────────────────────────────────────────────────

/// Parse views from the PRD-specified TOML format.
///
/// Format: `[views.<key>]` sections with name, type, fields, sort, filter, etc.
fn parse_views_table(table: &toml::Table) -> Result<Vec<BaseView>, BasesError> {
    let mut views = Vec::new();
    if let Some(toml::Value::Table(views_table)) = table.get("views") {
        for (_key, value) in views_table {
            let json = toml_value_to_json(value);
            let view: BaseView =
                serde_json::from_value(json).map_err(|e| BasesError::CorruptFile {
                    path: "views.toml".to_string(),
                    reason: e.to_string(),
                })?;
            views.push(view);
        }
    }
    Ok(views)
}

/// Parse relations from TOML format.
fn parse_relations_table(table: &toml::Table) -> Result<Vec<BaseRelation>, BasesError> {
    let mut relations = Vec::new();
    if let Some(toml::Value::Table(rels_table)) = table.get("relations") {
        for (key, value) in rels_table {
            let mut json = toml_value_to_json(value);
            // Inject the key as the name if not present.
            if let serde_json::Value::Object(ref mut map) = json {
                map.entry("name".to_string())
                    .or_insert_with(|| serde_json::Value::String(key.clone()));
            }
            let rel: BaseRelation =
                serde_json::from_value(json).map_err(|e| BasesError::CorruptFile {
                    path: "relations.toml".to_string(),
                    reason: e.to_string(),
                })?;
            relations.push(rel);
        }
    }
    Ok(relations)
}

/// Serialize views to the PRD-specified TOML format.
fn serialize_views_toml(views: &[BaseView]) -> Result<String, BasesError> {
    let mut table = toml::Table::new();
    let mut views_table = toml::Table::new();
    for (i, view) in views.iter().enumerate() {
        let key = view
            .name
            .to_lowercase()
            .replace(' ', "-")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect::<String>();
        let key = if key.is_empty() {
            format!("view-{i}")
        } else {
            key
        };
        let json = serde_json::to_value(view).map_err(|e| BasesError::CorruptFile {
            path: "views.toml".to_string(),
            reason: e.to_string(),
        })?;
        views_table.insert(key, json_to_toml_value(&json));
    }
    table.insert("views".to_string(), toml::Value::Table(views_table));
    toml::to_string_pretty(&table).map_err(|e| BasesError::CorruptFile {
        path: "views.toml".to_string(),
        reason: e.to_string(),
    })
}

/// Serialize relations to TOML format.
fn serialize_relations_toml(relations: &[BaseRelation]) -> Result<String, BasesError> {
    let mut table = toml::Table::new();
    let mut rels_table = toml::Table::new();
    for rel in relations {
        let json = serde_json::to_value(rel).map_err(|e| BasesError::CorruptFile {
            path: "relations.toml".to_string(),
            reason: e.to_string(),
        })?;
        rels_table.insert(rel.name.clone(), json_to_toml_value(&json));
    }
    table.insert("relations".to_string(), toml::Value::Table(rels_table));
    toml::to_string_pretty(&table).map_err(|e| BasesError::CorruptFile {
        path: "relations.toml".to_string(),
        reason: e.to_string(),
    })
}

/// Convert a TOML value to a JSON value.
fn toml_value_to_json(value: &toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_json::Value::Number((*i).into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(*f)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(toml_value_to_json).collect())
        }
        toml::Value::Table(t) => {
            let map: serde_json::Map<String, serde_json::Value> = t
                .iter()
                .map(|(k, v)| (k.clone(), toml_value_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
    }
}

/// Convert a JSON value to a TOML value.
fn json_to_toml_value(value: &serde_json::Value) -> toml::Value {
    match value {
        serde_json::Value::Null => toml::Value::String(String::new()),
        serde_json::Value::Bool(b) => toml::Value::Boolean(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                toml::Value::Integer(i)
            } else {
                toml::Value::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => toml::Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            toml::Value::Array(arr.iter().map(json_to_toml_value).collect())
        }
        serde_json::Value::Object(map) => {
            let t: toml::Table = map
                .iter()
                .map(|(k, v)| (k.clone(), json_to_toml_value(v)))
                .collect();
            toml::Value::Table(t)
        }
    }
}

// ── Utils ────────────────────────────────────────────────────────────────────

fn unix_now() -> i64 {
    #[allow(clippy::cast_possible_wrap)]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn sample_schema() -> BaseSchema {
        let mut fields = serde_json::Map::new();
        fields.insert(
            "id".to_string(),
            serde_json::json!({"type": "uuid", "primary": true}),
        );
        fields.insert(
            "title".to_string(),
            serde_json::json!({"type": "text", "required": true}),
        );
        fields.insert(
            "status".to_string(),
            serde_json::json!({"type": "select", "options": ["todo", "done"]}),
        );
        fields.insert(
            "priority".to_string(),
            serde_json::json!({"type": "number", "min": 1, "max": 5}),
        );
        BaseSchema {
            version: "1.0".to_string(),
            fields,
        }
    }

    fn sample_record(id: &str, title: &str, status: &str) -> BaseRecord {
        let mut fields = serde_json::Map::new();
        fields.insert("title".to_string(), serde_json::json!(title));
        fields.insert("status".to_string(), serde_json::json!(status));
        BaseRecord {
            id: id.to_string(),
            deleted_at: None,
            fields,
        }
    }

    #[test]
    fn parse_schema_json() {
        let json = r#"{"version":"1.0","fields":{"id":{"type":"uuid","primary":true},"title":{"type":"text"}}}"#;
        let schema: BaseSchema = serde_json::from_str(json).unwrap();
        assert_eq!(schema.version, "1.0");
        assert_eq!(schema.fields.len(), 2);
    }

    #[test]
    fn parse_records_json() {
        let json = r#"[{"id":"r1","title":"Task 1"},{"id":"r2","title":"Task 2"}]"#;
        let records: Vec<BaseRecord> = serde_json::from_str(json).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].id, "r1");
    }

    #[test]
    fn parse_views_toml_format() {
        let toml_str = r#"
[views.all-tasks]
name = "All Tasks"
type = "table"
fields = ["title", "status", "priority"]

[views.by-status]
name = "By Status"
type = "kanban"
groupField = "status"
fields = ["title", "priority"]
"#;
        let table: toml::Table = toml::from_str(toml_str).unwrap();
        let views = parse_views_table(&table).unwrap();
        assert_eq!(views.len(), 2);
        let table_view = views.iter().find(|v| v.name == "All Tasks").unwrap();
        assert_eq!(table_view.view_type, ViewType::Table);
        assert_eq!(table_view.fields.len(), 3);
        let kanban_view = views.iter().find(|v| v.name == "By Status").unwrap();
        assert_eq!(kanban_view.view_type, ViewType::Kanban);
        assert_eq!(kanban_view.group_field.as_deref(), Some("status"));
    }

    #[test]
    fn parse_relations_toml_format() {
        let toml_str = r#"
[relations.task-assignee]
type = "many-to-one"
sourceField = "assignee"
targetBase = "Users.bases"
targetField = "id"
"#;
        let table: toml::Table = toml::from_str(toml_str).unwrap();
        let relations = parse_relations_table(&table).unwrap();
        assert_eq!(relations.len(), 1);
        assert_eq!(relations[0].name, "task-assignee");
        assert_eq!(relations[0].relation_type, "many-to-one");
        assert_eq!(relations[0].target_base, "Users.bases");
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tmp();
        let base_dir = dir.path().join("Tasks.bases");
        let schema = sample_schema();
        let mut base = init_base(&base_dir, "Tasks", &schema).unwrap();
        base.records.push(sample_record("r1", "Buy milk", "todo"));
        base.records.push(sample_record("r2", "Write tests", "done"));
        base.views.push(BaseView {
            name: "All".to_string(),
            view_type: ViewType::Table,
            fields: vec!["title".to_string(), "status".to_string()],
            sort: vec![],
            filter: vec![],
            group_field: None,
            date_field: None,
            end_field: None,
        });
        save_base(&base_dir, &base).unwrap();

        let loaded = load_base(&base_dir).unwrap();
        assert_eq!(loaded.name, "Tasks");
        assert_eq!(loaded.records.len(), 2);
        assert_eq!(loaded.records[0].id, "r1");
        assert_eq!(loaded.views.len(), 1);
    }

    #[test]
    fn validate_record_valid() {
        let schema = sample_schema();
        let record = sample_record("r1", "Valid", "todo");
        assert!(validate_record(&schema, &record).is_ok());
    }

    #[test]
    fn validate_record_missing_required() {
        let schema = sample_schema();
        let record = BaseRecord {
            id: "r1".to_string(),
            deleted_at: None,
            fields: serde_json::Map::new(), // missing "title" which is required
        };
        let result = validate_record(&schema, &record);
        assert!(matches!(result, Err(BasesError::ValidationFailed(_))));
    }

    #[test]
    fn field_type_serde() {
        let ft = FieldType::MultiSelect;
        let json = serde_json::to_string(&ft).unwrap();
        assert_eq!(json, r#""multi-select""#);
        let parsed: FieldType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, FieldType::MultiSelect);
    }

    #[test]
    fn load_base_missing_schema_errors() {
        let dir = tmp();
        let base_dir = dir.path().join("Empty.bases");
        std::fs::create_dir_all(&base_dir).unwrap();
        let result = load_base(&base_dir);
        assert!(matches!(result, Err(BasesError::FileNotFound(_))));
    }

    #[test]
    fn init_base_creates_directory() {
        let dir = tmp();
        let base_dir = dir.path().join("New.bases");
        let schema = sample_schema();
        let base = init_base(&base_dir, "New", &schema).unwrap();
        assert_eq!(base.name, "New");
        assert!(base_dir.join("schema.json").exists());
        assert!(base_dir.join("records.json").exists());
        assert!(base_dir.join("metadata.json").exists());
    }

    /// Load each committed fixture (`fixtures/bases/*.bases`) to guard
    /// against schema / serialization drift — if someone renames a
    /// `ViewType` variant or tightens `FieldDefinition` without
    /// updating the fixtures, this test fails before the regression
    /// ships to users.
    #[test]
    fn committed_fixtures_round_trip_through_load_base() {
        // Walk up from CARGO_MANIFEST_DIR (= crates/nexus-types) to the
        // repo root so this works regardless of the cwd the test is
        // launched from.
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_dir
            .parent()
            .and_then(|p| p.parent())
            .expect("repo root");
        let fixtures = repo_root.join("fixtures").join("bases");
        if !fixtures.exists() {
            // Shallow clones might skip the fixtures directory — treat
            // as a benign skip instead of a hard failure.
            eprintln!("skipping: fixtures directory absent at {}", fixtures.display());
            return;
        }
        let expected = ["Tasks.bases", "Books.bases", "Contacts.bases"];
        for name in expected {
            let dir = fixtures.join(name);
            assert!(
                dir.exists(),
                "committed fixture missing: {}",
                dir.display(),
            );
            let base = load_base(&dir).unwrap_or_else(|e| {
                panic!("fixture '{name}' failed to load: {e}")
            });
            assert!(!base.records.is_empty(), "{name}: records empty");
            assert!(!base.views.is_empty(), "{name}: no views configured");
            // Every record must validate against the fixture's schema.
            for record in &base.records {
                validate_record(&base.schema, record).unwrap_or_else(|e| {
                    panic!(
                        "fixture '{name}' record '{}' failed validation: {e}",
                        record.id,
                    )
                });
            }
        }
    }
}
