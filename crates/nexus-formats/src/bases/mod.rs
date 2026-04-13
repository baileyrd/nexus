//! Bases database format loader and saver.
//!
//! A `.bases` path is a directory with five files:
//! - `schema.json`    — field definitions
//! - `records.json`   — data records
//! - `views.toml`     — view definitions
//! - `relations.toml` — relation definitions
//! - `metadata.json`  — version, timestamps

mod records;
mod schema;
mod views;

pub use records::BaseRecord;
pub use schema::{BaseSchema, FieldDefinition, FieldType};
pub use views::{
    BaseMetadata, BaseRelation, BaseView, RelationType, RelationsFile, SortDirection, SortRule,
    ViewType, ViewsFile,
};

use std::path::Path;

use crate::error::BasesError;
use crate::version::FormatVersion;

// ── Public type ───────────────────────────────────────────────────────────────

/// A fully loaded base database.
#[derive(Debug, Clone)]
pub struct Base {
    /// Field schema.
    pub schema: BaseSchema,
    /// All data records.
    pub records: Vec<BaseRecord>,
    /// View definitions.
    pub views: ViewsFile,
    /// Relation definitions.
    pub relations: RelationsFile,
    /// File metadata.
    pub metadata: BaseMetadata,
}

// ── Load / Save ───────────────────────────────────────────────────────────────

/// Load a base from a `.bases` directory.
///
/// `schema.json` and `records.json` are required.
/// `views.toml`, `relations.toml`, and `metadata.json` return defaults when absent.
///
/// # Errors
///
/// - [`BasesError::MissingFile`] if `schema.json` or `records.json` is absent.
/// - [`BasesError::CorruptFile`] on parse failure.
/// - [`BasesError::VersionMismatch`] if `metadata.json` declares an incompatible version.
pub fn load(dir: &Path) -> Result<Base, BasesError> {
    let schema   = load_required_json::<BaseSchema>(dir, "schema.json")?;
    let records  = load_required_json::<Vec<BaseRecord>>(dir, "records.json")?;
    let metadata = load_optional_json::<BaseMetadata>(dir, "metadata.json")?
        .unwrap_or_default();
    let views     = load_optional_toml::<ViewsFile>(dir, "views.toml")?
        .unwrap_or_default();
    let relations = load_optional_toml::<RelationsFile>(dir, "relations.toml")?
        .unwrap_or_default();

    // Version check on metadata.
    check_version(&metadata.version, &dir.join("metadata.json").display().to_string())?;

    Ok(Base { schema, records, views, relations, metadata })
}

/// Save a base to a `.bases` directory.
///
/// Creates the directory if it does not exist.
///
/// # Errors
///
/// Returns [`BasesError::CorruptFile`] on serialization or I/O failure.
pub fn save(dir: &Path, base: &Base) -> Result<(), BasesError> {
    std::fs::create_dir_all(dir).map_err(|e| BasesError::CorruptFile {
        path: dir.display().to_string(),
        reason: e.to_string(),
    })?;

    write_json(dir, "schema.json", &base.schema)?;
    write_json(dir, "records.json", &base.records)?;
    write_json(dir, "metadata.json", &base.metadata)?;
    write_toml(dir, "views.toml", &base.views)?;
    write_toml(dir, "relations.toml", &base.relations)?;

    Ok(())
}

/// Validate that a record's required fields are present.
///
/// # Errors
///
/// Returns [`BasesError::CorruptFile`] describing the first missing required field.
pub fn validate_record(schema: &BaseSchema, record: &BaseRecord) -> Result<(), BasesError> {
    for (name, def) in &schema.fields {
        if def.required && record.get(name).is_none_or(serde_json::Value::is_null) {
            return Err(BasesError::CorruptFile {
                path: "<record>".to_string(),
                reason: format!("required field '{name}' is missing or null"),
            });
        }
    }
    Ok(())
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn load_required_json<T>(dir: &Path, filename: &str) -> Result<T, BasesError>
where
    T: serde::de::DeserializeOwned,
{
    let path = dir.join(filename);
    if !path.exists() {
        return Err(BasesError::MissingFile { path: path.display().to_string() });
    }
    let text = std::fs::read_to_string(&path).map_err(|e| BasesError::CorruptFile {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    serde_json::from_str(&text).map_err(|e| BasesError::CorruptFile {
        path: path.display().to_string(),
        reason: e.to_string(),
    })
}

fn load_optional_json<T>(dir: &Path, filename: &str) -> Result<Option<T>, BasesError>
where
    T: serde::de::DeserializeOwned,
{
    let path = dir.join(filename);
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| BasesError::CorruptFile {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| BasesError::CorruptFile {
            path: path.display().to_string(),
            reason: e.to_string(),
        })
}

fn load_optional_toml<T>(dir: &Path, filename: &str) -> Result<Option<T>, BasesError>
where
    T: serde::de::DeserializeOwned,
{
    let path = dir.join(filename);
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| BasesError::CorruptFile {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    toml::from_str(&text)
        .map(Some)
        .map_err(|e| BasesError::CorruptFile {
            path: path.display().to_string(),
            reason: e.to_string(),
        })
}

fn write_json<T: serde::Serialize>(dir: &Path, filename: &str, value: &T) -> Result<(), BasesError> {
    let path = dir.join(filename);
    let text = serde_json::to_string_pretty(value).map_err(|e| BasesError::CorruptFile {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    std::fs::write(&path, text).map_err(|e| BasesError::CorruptFile {
        path: path.display().to_string(),
        reason: e.to_string(),
    })
}

fn write_toml<T: serde::Serialize>(dir: &Path, filename: &str, value: &T) -> Result<(), BasesError> {
    let path = dir.join(filename);
    let text = toml::to_string_pretty(value).map_err(|e| BasesError::CorruptFile {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    std::fs::write(&path, text).map_err(|e| BasesError::CorruptFile {
        path: path.display().to_string(),
        reason: e.to_string(),
    })
}

fn check_version(version_str: &str, path: &str) -> Result<(), BasesError> {
    let v = FormatVersion::parse(version_str).map_err(|_| BasesError::VersionMismatch {
        path: path.to_string(),
        found: version_str.to_string(),
    })?;
    let current = FormatVersion(1, 0, 0);
    if !current.is_compatible_with(&FormatVersion(v.major(), 0, 0)) {
        return Err(BasesError::VersionMismatch {
            path: path.to_string(),
            found: version_str.to_string(),
        });
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_minimal_base(dir: &Path) {
        let schema = serde_json::json!({
            "version": "1.0",
            "fields": {
                "id": { "type": "uuid", "primary": true },
                "title": { "type": "text", "required": true }
            }
        });
        let records = serde_json::json!([
            { "id": "uuid-1", "title": "Task One" },
            { "id": "uuid-2", "title": "Task Two" }
        ]);
        std::fs::write(dir.join("schema.json"), schema.to_string()).unwrap();
        std::fs::write(dir.join("records.json"), records.to_string()).unwrap();
    }

    #[test]
    fn load_minimal_base() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("tasks.bases");
        std::fs::create_dir(&dir).unwrap();
        make_minimal_base(&dir);

        let base = load(&dir).unwrap();
        assert_eq!(base.records.len(), 2);
        assert!(base.schema.fields.contains_key("id"));
        assert!(base.schema.fields.contains_key("title"));
    }

    #[test]
    fn missing_schema_returns_error() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("bad.bases");
        std::fs::create_dir(&dir).unwrap();
        // Only records, no schema.
        std::fs::write(dir.join("records.json"), "[]").unwrap();

        let err = load(&dir).unwrap_err();
        assert!(matches!(err, BasesError::MissingFile { .. }));
    }

    #[test]
    fn save_and_load_round_trip() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("test.bases");

        let base = Base {
            schema: BaseSchema::default(),
            records: vec![BaseRecord::new({
                let mut m = serde_json::Map::new();
                m.insert("id".into(), "r1".into());
                m.insert("name".into(), "Test".into());
                m
            })],
            views: ViewsFile::default(),
            relations: RelationsFile::default(),
            metadata: BaseMetadata {
                version: "1.0".to_string(),
                created: Some("2026-04-13T00:00:00Z".to_string()),
                modified: None,
                name: Some("TestDB".to_string()),
            },
        };

        save(&dir, &base).unwrap();
        let loaded = load(&dir).unwrap();
        assert_eq!(loaded.records.len(), 1);
        assert_eq!(loaded.records[0].id(), Some("r1"));
        assert_eq!(loaded.metadata.name.as_deref(), Some("TestDB"));
    }

    #[test]
    fn version_mismatch_returns_error() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("future.bases");
        std::fs::create_dir(&dir).unwrap();
        make_minimal_base(&dir);
        // Write metadata with a future major version.
        let meta = serde_json::json!({"version": "2.0"});
        std::fs::write(dir.join("metadata.json"), meta.to_string()).unwrap();

        let err = load(&dir).unwrap_err();
        assert!(matches!(err, BasesError::VersionMismatch { .. }));
    }

    #[test]
    fn validate_record_required_field_missing() {
        let mut schema = BaseSchema::default();
        schema.fields.insert("title".into(), FieldDefinition {
            field_type: FieldType::Text,
            required: true,
            primary: false,
            options: None,
            min: None, max: None, target: None, target_field: None,
        });

        let record = BaseRecord::new(serde_json::Map::new()); // no 'title'
        let err = validate_record(&schema, &record).unwrap_err();
        assert!(matches!(err, BasesError::CorruptFile { .. }));
    }

    #[test]
    fn validate_record_ok() {
        let mut schema = BaseSchema::default();
        schema.fields.insert("title".into(), FieldDefinition {
            field_type: FieldType::Text,
            required: true,
            primary: false,
            options: None,
            min: None, max: None, target: None, target_field: None,
        });

        let mut map = serde_json::Map::new();
        map.insert("title".into(), "Hello".into());
        let record = BaseRecord::new(map);
        assert!(validate_record(&schema, &record).is_ok());
    }
}
