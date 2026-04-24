//! `SQLite` persistence + query/schema/relation engine for `.bases` databases.
//!
//! The on-disk `.bases` directory format (parser, serializer, validation) and
//! the shared type set live in [`nexus_types::bases`]; this module re-exports
//! them for backwards compatibility and provides the `SQLite` index
//! operations (`insert_base`, `query_bases`, `delete_base`) that require a
//! rusqlite connection.
//!
//! Submodules [`schema`], [`query`], and [`relation`] host the full query
//! engine (migrations, SELECT execution, relation resolution, rollup
//! aggregation). They were previously in `nexus-database`; moving them here
//! consolidates all `rusqlite` access into this crate — there is one owner
//! of the forge's `SQLite` database, and it's `nexus-storage`. Pure-logic
//! types, validators, formulas, and CSV import/export remain in
//! `nexus-database` as a no-rusqlite library.

pub mod query;
pub mod relation;
pub mod schema;

use rusqlite::Connection;

use crate::StorageError;

// ── Re-exports ───────────────────────────────────────────────────────────────
//
// The filesystem layer, types, and validation live in `nexus-types` so
// `nexus-database`, the CLI, and other non-storage consumers can use them
// without pulling in this crate.

pub use nexus_types::bases::{
    Base, BaseMetadata, BaseRecord, BaseRelation, BaseSchema, BaseSummary, BaseView, BasesError,
    FieldDefinition, FieldType, FilterRule, SortRule, ViewType, init_base, load_base, save_base,
    validate_record,
};

// ── DB Operations ────────────────────────────────────────────────────────────

/// Insert a base and its records into `SQLite`.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn insert_base(conn: &Connection, path: &str, base: &Base) -> Result<i64, StorageError> {
    let schema_json = serde_json::to_string(&base.schema).map_err(|e| StorageError::CorruptFile {
        path: path.to_string(),
        reason: e.to_string(),
    })?;
    let metadata_json = serde_json::to_string(&base.metadata).ok();
    let now = crate::unix_now();

    conn.execute(
        "INSERT OR REPLACE INTO bases (path, name, schema_json, metadata_json, created_at, modified_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
        rusqlite::params![path, base.name, schema_json, metadata_json, now, now],
    )?;
    let base_id = conn.last_insert_rowid();

    // Delete existing records for this base (in case of re-insert).
    conn.execute(
        "DELETE FROM bases_records WHERE base_id = ?1;",
        rusqlite::params![base_id],
    )?;

    // Insert records.
    let mut stmt = conn.prepare_cached(
        "INSERT INTO bases_records (base_id, record_id, data_json, created_at, modified_at)
         VALUES (?1, ?2, ?3, ?4, ?5);",
    )?;
    for record in &base.records {
        let data = serde_json::to_string(&record).unwrap_or_default();
        stmt.execute(rusqlite::params![base_id, record.id, data, now, now])?;
    }

    // Delete and re-insert views.
    conn.execute(
        "DELETE FROM bases_views WHERE base_id = ?1;",
        rusqlite::params![base_id],
    )?;
    let mut view_stmt = conn.prepare_cached(
        "INSERT INTO bases_views (base_id, name, view_type, config_json)
         VALUES (?1, ?2, ?3, ?4);",
    )?;
    for view in &base.views {
        let config = serde_json::to_string(view).unwrap_or_default();
        let vt = match view.view_type {
            ViewType::Table => "table",
            ViewType::Kanban => "kanban",
            ViewType::Calendar => "calendar",
            ViewType::Gallery => "gallery",
            ViewType::List => "list",
            ViewType::Timeline => "timeline",
        };
        view_stmt.execute(rusqlite::params![base_id, view.name, vt, config])?;
    }

    Ok(base_id)
}

/// List all bases in the index.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn query_bases(conn: &Connection) -> Result<Vec<BaseSummary>, StorageError> {
    let mut stmt = conn.prepare_cached(
        "SELECT b.id, b.path, b.name,
                (SELECT COUNT(*) FROM bases_records r WHERE r.base_id = b.id) as cnt
         FROM bases b ORDER BY b.path;",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(BaseSummary {
            id: row.get(0)?,
            path: row.get(1)?,
            name: row.get(2)?,
            record_count: row.get(3)?,
        })
    })?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Delete a base and its records from the index.
///
/// # Errors
///
/// Returns [`StorageError::Database`] on any `SQLite` failure.
pub fn delete_base(conn: &Connection, base_id: i64) -> Result<(), StorageError> {
    conn.execute(
        "DELETE FROM bases WHERE id = ?1;",
        rusqlite::params![base_id],
    )?;
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
        BaseSchema {
            version: "1.0".to_string(),
            fields,
        }
    }

    fn sample_record(id: &str, title: &str) -> BaseRecord {
        let mut fields = serde_json::Map::new();
        fields.insert("title".to_string(), serde_json::json!(title));
        BaseRecord {
            id: id.to_string(),
            deleted_at: None,
            fields,
        }
    }

    #[test]
    fn insert_and_query_base() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::configure_pragmas(&conn).unwrap();
        crate::schema::migrate(&conn).unwrap();

        let base = Base {
            name: "Tasks".to_string(),
            schema: sample_schema(),
            records: vec![
                sample_record("r1", "Task 1"),
                sample_record("r2", "Task 2"),
            ],
            views: vec![BaseView {
                name: "All".to_string(),
                view_type: ViewType::Table,
                fields: vec!["title".to_string()],
                sort: vec![],
                filter: vec![],
                group_field: None,
                date_field: None,
                end_field: None,
            }],
            relations: vec![],
            metadata: BaseMetadata::default(),
        };

        let base_id = insert_base(&conn, "Tasks.bases", &base).unwrap();
        assert!(base_id > 0);

        let summaries = query_bases(&conn).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].name, "Tasks");
        assert_eq!(summaries[0].record_count, 2);
    }

    #[test]
    fn delete_base_removes_records() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::configure_pragmas(&conn).unwrap();
        crate::schema::migrate(&conn).unwrap();

        let base = Base {
            name: "Del".to_string(),
            schema: sample_schema(),
            records: vec![sample_record("r1", "Temp")],
            views: vec![],
            relations: vec![],
            metadata: BaseMetadata::default(),
        };
        let base_id = insert_base(&conn, "Del.bases", &base).unwrap();
        delete_base(&conn, base_id).unwrap();

        let summaries = query_bases(&conn).unwrap();
        assert!(summaries.is_empty());
    }
}
