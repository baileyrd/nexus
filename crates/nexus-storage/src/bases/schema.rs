//! Schema versioning and migration tracking for databases.
//!
//! Each schema change (add/remove/rename property, change type) produces a
//! [`SchemaMigration`] record stored in the `bases_schema_versions` `SQLite` table.
//! This enables migration history, undo, and compatibility checks.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use nexus_database::{DatabaseError, PropertyConfig, Result};

// ── Migration types ─────────────────────────────────────────────────────────

/// A single schema migration operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SchemaOp {
    /// Add a new property to the schema.
    AddProperty {
        /// Property (field) name.
        name: String,
        /// Property configuration.
        config: PropertyConfig,
    },
    /// Remove a property from the schema.
    RemoveProperty {
        /// Property name to remove.
        name: String,
    },
    /// Rename a property (data preserved, ID-based).
    RenameProperty {
        /// Current name.
        old_name: String,
        /// New name.
        new_name: String,
    },
    /// Modify a property's configuration (e.g., add options, change bounds).
    ModifyConfig {
        /// Property name.
        name: String,
        /// New configuration (replaces the old one).
        config: PropertyConfig,
    },
}

/// A recorded schema migration with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaMigration {
    /// Auto-incremented database row ID.
    pub id: i64,
    /// The base this migration belongs to.
    pub base_id: i64,
    /// Monotonically increasing version number within this base.
    pub version: u32,
    /// The operation that was applied.
    pub operation: SchemaOp,
    /// Unix timestamp when the migration was applied.
    pub applied_at: i64,
}

// ── Migration application ───────────────────────────────────────────────────

/// Apply a schema operation to a base.
///
/// 1. Loads the current `.bases` directory from disk
/// 2. Modifies the schema according to `op`
/// 3. Saves the updated `.bases` directory
/// 4. Re-indexes into `SQLite`
/// 5. Records the migration in `bases_schema_versions`
///
/// # Errors
///
/// Returns `DatabaseError::SchemaError` if the operation is invalid (e.g.,
/// removing a non-existent property), or propagates storage errors.
pub fn apply_migration(
    conn: &Connection,
    base_dir: &Path,
    base_id: i64,
    op: SchemaOp,
) -> Result<SchemaMigration> {
    // Load current state.
    let mut base = nexus_types::bases::load_base(base_dir)?;

    // Apply the operation to the in-memory schema.
    match &op {
        SchemaOp::AddProperty { name, config } => {
            if base.schema.fields.contains_key(name) {
                return Err(DatabaseError::SchemaError(format!(
                    "property '{name}' already exists"
                )));
            }
            let config_json = serde_json::to_value(config).map_err(|e| {
                DatabaseError::SchemaError(format!("failed to serialize config: {e}"))
            })?;
            base.schema.fields.insert(name.clone(), config_json);
        }
        SchemaOp::RemoveProperty { name } => {
            if base.schema.fields.remove(name).is_none() {
                return Err(DatabaseError::SchemaError(format!(
                    "property '{name}' does not exist"
                )));
            }
            // Remove the property value from all records.
            for record in &mut base.records {
                record.fields.remove(name);
            }
        }
        SchemaOp::RenameProperty { old_name, new_name } => {
            let value = base.schema.fields.remove(old_name).ok_or_else(|| {
                DatabaseError::SchemaError(format!("property '{old_name}' does not exist"))
            })?;
            base.schema.fields.insert(new_name.clone(), value);
            // Rename in all records.
            for record in &mut base.records {
                if let Some(v) = record.fields.remove(old_name) {
                    record.fields.insert(new_name.clone(), v);
                }
            }
        }
        SchemaOp::ModifyConfig { name, config } => {
            if !base.schema.fields.contains_key(name) {
                return Err(DatabaseError::SchemaError(format!(
                    "property '{name}' does not exist"
                )));
            }
            let config_json = serde_json::to_value(config).map_err(|e| {
                DatabaseError::SchemaError(format!("failed to serialize config: {e}"))
            })?;
            base.schema.fields.insert(name.clone(), config_json);
        }
    }

    // Persist to disk.
    nexus_types::bases::save_base(base_dir, &base)?;

    // Update the SQLite index in-place (UPDATE, not INSERT OR REPLACE)
    // to avoid cascade-deleting schema_versions records.
    let schema_json = serde_json::to_string(&base.schema)
        .map_err(|e| DatabaseError::SchemaError(format!("failed to serialize schema: {e}")))?;
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE bases SET schema_json = ?1, modified_at = ?2 WHERE id = ?3",
        rusqlite::params![schema_json, now, base_id],
    )
    .map_err(|e| DatabaseError::SchemaError(format!("failed to update index: {e}")))?;

    // Record the migration.
    let version = current_version(conn, base_id)? + 1;
    let op_json = serde_json::to_string(&op)
        .map_err(|e| DatabaseError::SchemaError(format!("failed to serialize operation: {e}")))?;

    conn.execute(
        "INSERT INTO bases_schema_versions (base_id, version, operation, applied_at)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![base_id, version, op_json, now],
    )
    .map_err(|e| DatabaseError::SchemaError(format!("failed to record migration: {e}")))?;

    let id = conn.last_insert_rowid();

    Ok(SchemaMigration {
        id,
        base_id,
        version,
        operation: op,
        applied_at: now,
    })
}

/// Get the current schema version number for a base.
///
/// Returns 0 if no migrations have been applied.
///
/// # Errors
///
/// Returns `DatabaseError::SchemaError` on `SQLite` failures.
pub fn current_version(conn: &Connection, base_id: i64) -> Result<u32> {
    let version: Option<u32> = conn
        .query_row(
            "SELECT MAX(version) FROM bases_schema_versions WHERE base_id = ?1",
            rusqlite::params![base_id],
            |row| row.get(0),
        )
        .map_err(|e| DatabaseError::SchemaError(format!("failed to query version: {e}")))?;
    Ok(version.unwrap_or(0))
}

/// Get the full migration history for a base, ordered by version.
///
/// # Errors
///
/// Returns `DatabaseError::SchemaError` on `SQLite` failures.
pub fn migration_history(conn: &Connection, base_id: i64) -> Result<Vec<SchemaMigration>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, base_id, version, operation, applied_at
             FROM bases_schema_versions
             WHERE base_id = ?1
             ORDER BY version ASC",
        )
        .map_err(|e| DatabaseError::SchemaError(format!("prepare failed: {e}")))?;

    let rows = stmt
        .query_map(rusqlite::params![base_id], |row| {
            let op_json: String = row.get(3)?;
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, u32>(2)?,
                op_json,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|e| DatabaseError::SchemaError(format!("query failed: {e}")))?;

    let mut migrations = Vec::new();
    for row in rows {
        let (id, base_id, version, op_json, applied_at) =
            row.map_err(|e| DatabaseError::SchemaError(format!("row read failed: {e}")))?;
        let operation: SchemaOp = serde_json::from_str(&op_json).map_err(|e| {
            DatabaseError::SchemaError(format!("failed to deserialize operation: {e}"))
        })?;
        migrations.push(SchemaMigration {
            id,
            base_id,
            version,
            operation,
            applied_at,
        });
    }

    Ok(migrations)
}

/// Extract [`PropertyConfig`] for each field from a `BaseSchema`.
///
/// Attempts to deserialize each field's JSON value into a `PropertyConfig`.
/// Fields that fail to deserialize are skipped with a warning.
#[must_use]
pub fn extract_configs(
    schema: &nexus_types::bases::BaseSchema,
) -> BTreeMap<String, PropertyConfig> {
    let mut configs = BTreeMap::new();
    for (name, value) in &schema.fields {
        match serde_json::from_value::<PropertyConfig>(value.clone()) {
            Ok(config) => {
                configs.insert(name.clone(), config);
            }
            Err(e) => {
                tracing::warn!(
                    field = name,
                    error = %e,
                    "failed to parse property config; skipping"
                );
            }
        }
    }
    configs
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::configure_pragmas(&conn).unwrap();
        crate::schema::migrate(&conn).unwrap();
        // Create the schema versions table (migration 007).
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS bases_schema_versions (
                id          INTEGER PRIMARY KEY,
                base_id     INTEGER NOT NULL,
                version     INTEGER NOT NULL,
                operation   TEXT NOT NULL,
                applied_at  INTEGER NOT NULL,
                FOREIGN KEY(base_id) REFERENCES bases(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_bsv_base ON bases_schema_versions(base_id);",
        )
        .unwrap();
        (dir, conn)
    }

    fn create_test_base(dir: &Path, conn: &Connection) -> i64 {
        let base_dir = dir.join("Test.bases");
        let schema = nexus_types::bases::BaseSchema {
            version: "1.0".to_string(),
            fields: serde_json::Map::new(),
        };
        let base = nexus_types::bases::init_base(&base_dir, "Test", &schema).unwrap();
        crate::bases::insert_base(conn, &base_dir.display().to_string(), &base).unwrap()
    }

    #[test]
    fn add_property_migration() {
        let (dir, conn) = setup_db();
        let base_id = create_test_base(dir.path(), &conn);
        let base_dir = dir.path().join("Test.bases");

        let migration = apply_migration(
            &conn,
            &base_dir,
            base_id,
            SchemaOp::AddProperty {
                name: "title".to_string(),
                config: PropertyConfig::Text { max_length: None },
            },
        )
        .unwrap();

        assert_eq!(migration.version, 1);
        assert_eq!(migration.base_id, base_id);

        // Verify the property was added to the file.
        let reloaded = nexus_types::bases::load_base(&base_dir).unwrap();
        assert!(reloaded.schema.fields.contains_key("title"));
    }

    #[test]
    fn add_duplicate_property_fails() {
        let (dir, conn) = setup_db();
        let base_id = create_test_base(dir.path(), &conn);
        let base_dir = dir.path().join("Test.bases");

        apply_migration(
            &conn,
            &base_dir,
            base_id,
            SchemaOp::AddProperty {
                name: "title".to_string(),
                config: PropertyConfig::Text { max_length: None },
            },
        )
        .unwrap();

        let result = apply_migration(
            &conn,
            &base_dir,
            base_id,
            SchemaOp::AddProperty {
                name: "title".to_string(),
                config: PropertyConfig::Text { max_length: None },
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn remove_property_migration() {
        let (dir, conn) = setup_db();
        let base_id = create_test_base(dir.path(), &conn);
        let base_dir = dir.path().join("Test.bases");

        apply_migration(
            &conn,
            &base_dir,
            base_id,
            SchemaOp::AddProperty {
                name: "temp".to_string(),
                config: PropertyConfig::Checkbox,
            },
        )
        .unwrap();

        let m = apply_migration(
            &conn,
            &base_dir,
            base_id,
            SchemaOp::RemoveProperty {
                name: "temp".to_string(),
            },
        )
        .unwrap();
        assert_eq!(m.version, 2);

        let reloaded = nexus_types::bases::load_base(&base_dir).unwrap();
        assert!(!reloaded.schema.fields.contains_key("temp"));
    }

    #[test]
    fn rename_property_migration() {
        let (dir, conn) = setup_db();
        let base_id = create_test_base(dir.path(), &conn);
        let base_dir = dir.path().join("Test.bases");

        apply_migration(
            &conn,
            &base_dir,
            base_id,
            SchemaOp::AddProperty {
                name: "old_name".to_string(),
                config: PropertyConfig::Text { max_length: None },
            },
        )
        .unwrap();

        apply_migration(
            &conn,
            &base_dir,
            base_id,
            SchemaOp::RenameProperty {
                old_name: "old_name".to_string(),
                new_name: "new_name".to_string(),
            },
        )
        .unwrap();

        let reloaded = nexus_types::bases::load_base(&base_dir).unwrap();
        assert!(!reloaded.schema.fields.contains_key("old_name"));
        assert!(reloaded.schema.fields.contains_key("new_name"));
    }

    #[test]
    fn migration_history_ordered() {
        let (dir, conn) = setup_db();
        let base_id = create_test_base(dir.path(), &conn);
        let base_dir = dir.path().join("Test.bases");

        for i in 0..3 {
            apply_migration(
                &conn,
                &base_dir,
                base_id,
                SchemaOp::AddProperty {
                    name: format!("field_{i}"),
                    config: PropertyConfig::Text { max_length: None },
                },
            )
            .unwrap();
        }

        let history = migration_history(&conn, base_id).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].version, 1);
        assert_eq!(history[1].version, 2);
        assert_eq!(history[2].version, 3);
    }

    #[test]
    fn current_version_starts_at_zero() {
        let (_dir, conn) = setup_db();
        // Non-existent base_id should return 0.
        assert_eq!(current_version(&conn, 999).unwrap(), 0);
    }
}
