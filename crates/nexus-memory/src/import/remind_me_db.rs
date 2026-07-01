//! Import memories from a `remind_me` `memory.db` into the native store.
//!
//! Reads the foreign `memories` table read-only and maps each row into a
//! native [`Memory`], preserving ids and timestamps so the import round-trips
//! 1:1 and is idempotent on id. Tolerant of older `remind_me` schema versions:
//! it probes `PRAGMA table_info` and only selects columns that exist, applying
//! [`Memory`] defaults for anything a given version lacks.

use std::collections::HashSet;
use std::path::Path;

use rusqlite::{Connection, OpenFlags, Row};

use crate::db::{parse_dt, parse_uuid, MemoryDb, Result};
use crate::model::{Memory, MemoryStatus, MemoryType};

/// Outcome of an import run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Memories written to the target store.
    pub imported: usize,
    /// Source rows skipped because they failed to map.
    pub skipped: usize,
}

/// Columns added by later `remind_me` migrations — selected only when present.
const OPTIONAL_COLS: &[&str] = &[
    "client",
    "capture_id",
    "source_capture_id",
    "memory_type",
    "status",
    "superseded_by",
    "subject",
    "predicate",
    "object",
    "accessed_at",
    "access_count",
    "decay_rate",
    "vitality",
    "base_weight",
];

/// Import every row from a `remind_me` `memory.db` at `source` into `target`.
///
/// Ids and timestamps are preserved. Embeddings are **not** copied — vectors
/// are recomputed by the `nexus-ai` path on demand (design D-1).
///
/// # Errors
/// Returns an error if the source database cannot be opened read-only or its
/// `memories` table cannot be read.
pub fn import_remind_me_db(target: &MemoryDb, source: &Path) -> Result<ImportReport> {
    let conn = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let cols = table_columns(&conn, "memories")?;

    let mut select = vec![
        "id",
        "content",
        "category",
        "tags",
        "source",
        "metadata",
        "created_at",
        "updated_at",
    ];
    select.extend(OPTIONAL_COLS.iter().copied().filter(|c| cols.contains(*c)));
    let sql = format!("SELECT {} FROM memories", select.join(", "));

    let mut stmt = conn.prepare(&sql)?;
    let mut report = ImportReport::default();
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        match map_row(row, &cols) {
            Ok(mem) => {
                target.insert(&mem)?;
                report.imported += 1;
            }
            Err(_) => report.skipped += 1,
        }
    }
    Ok(report)
}

/// The set of column names on `table` in the source database.
fn table_columns(conn: &Connection, table: &str) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = stmt
        .query_map([], |r| r.get::<_, String>("name"))?
        .collect::<rusqlite::Result<HashSet<String>>>()?;
    Ok(names)
}

/// Read an optional string column, returning `None` when the column is absent
/// from this `remind_me` schema version or NULL in the row.
fn opt_str(row: &Row<'_>, cols: &HashSet<String>, name: &str) -> rusqlite::Result<Option<String>> {
    if cols.contains(name) {
        row.get(name)
    } else {
        Ok(None)
    }
}

fn map_row(row: &Row<'_>, cols: &HashSet<String>) -> Result<Memory> {
    let mut m = Memory::new(row.get::<_, String>("content")?);
    m.id = parse_uuid(&row.get::<_, String>("id")?)?;
    m.category = row.get("category")?;
    if let Ok(tags) = serde_json::from_str::<Vec<String>>(&row.get::<_, String>("tags")?) {
        m.tags = tags;
    }
    m.source = row.get("source")?;
    m.metadata = serde_json::from_str::<serde_json::Value>(&row.get::<_, String>("metadata")?)
        .unwrap_or_else(|_| serde_json::json!({}));
    m.created_at = parse_dt(&row.get::<_, String>("created_at")?)?;
    m.updated_at = parse_dt(&row.get::<_, String>("updated_at")?)?;

    if let Some(v) = opt_str(row, cols, "client")? {
        m.client = v;
    }
    m.capture_id = opt_str(row, cols, "capture_id")?;
    m.source_capture_id = opt_str(row, cols, "source_capture_id")?;
    if let Some(v) = opt_str(row, cols, "memory_type")? {
        m.memory_type = MemoryType::from_db(&v);
    }
    if let Some(v) = opt_str(row, cols, "status")? {
        m.status = MemoryStatus::from_db(&v);
    }
    m.superseded_by = match opt_str(row, cols, "superseded_by")? {
        Some(s) => Some(parse_uuid(&s)?),
        None => None,
    };
    m.subject = opt_str(row, cols, "subject")?;
    m.predicate = opt_str(row, cols, "predicate")?;
    m.object = opt_str(row, cols, "object")?;
    m.accessed_at = match opt_str(row, cols, "accessed_at")? {
        Some(s) => Some(parse_dt(&s)?),
        None => None,
    };
    if cols.contains("access_count") {
        m.access_count = row.get("access_count")?;
    }
    if cols.contains("decay_rate") {
        m.decay_rate = row.get("decay_rate")?;
    }
    if cols.contains("vitality") {
        m.vitality = row.get("vitality")?;
    }
    if cols.contains("base_weight") {
        m.base_weight = row.get("base_weight")?;
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    /// Build a `remind_me`-shaped source database. When `full`, include the
    /// later-migration columns; otherwise only the base v1 columns.
    fn make_source(path: &Path, full: bool) {
        let conn = Connection::open(path).unwrap();
        if full {
            conn.execute_batch(
                "CREATE TABLE memories (
                    id TEXT PRIMARY KEY, content TEXT NOT NULL,
                    category TEXT NOT NULL DEFAULT 'general', tags TEXT NOT NULL DEFAULT '[]',
                    source TEXT NOT NULL DEFAULT 'manual', metadata TEXT NOT NULL DEFAULT '{}',
                    created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
                    client TEXT, memory_type TEXT, status TEXT, subject TEXT,
                    vitality REAL, access_count INTEGER);",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO memories (id, content, category, tags, source, metadata, created_at, \
                 updated_at, client, memory_type, status, subject, vitality, access_count) \
                 VALUES (?1, ?2, 'pref', '[\"a\"]', 'import', '{}', \
                 '2026-01-01T00:00:00Z', '2026-01-02T00:00:00Z', 'claude', 'semantic', 'active', \
                 'user', 0.7, 3)",
                rusqlite::params![Uuid::now_v7().to_string(), "user prefers dark mode"],
            )
            .unwrap();
        } else {
            conn.execute_batch(
                "CREATE TABLE memories (
                    id TEXT PRIMARY KEY, content TEXT NOT NULL,
                    category TEXT NOT NULL DEFAULT 'general', tags TEXT NOT NULL DEFAULT '[]',
                    source TEXT NOT NULL DEFAULT 'manual', metadata TEXT NOT NULL DEFAULT '{}',
                    created_at TEXT NOT NULL, updated_at TEXT NOT NULL);",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO memories (id, content, created_at, updated_at) \
                 VALUES (?1, ?2, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                rusqlite::params![Uuid::now_v7().to_string(), "minimal note about cats"],
            )
            .unwrap();
        }
    }

    #[test]
    fn imports_full_schema_row() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("memory.db");
        make_source(&src, true);
        let target = MemoryDb::open_in_memory().unwrap();
        let report = import_remind_me_db(&target, &src).unwrap();
        assert_eq!(report.imported, 1);
        assert_eq!(report.skipped, 0);
        let hits = target.search("dark mode", 10).unwrap();
        assert_eq!(hits.len(), 1);
        let m = &hits[0];
        assert_eq!(m.category, "pref");
        assert_eq!(m.client, "claude");
        assert_eq!(m.memory_type, MemoryType::Semantic);
        assert_eq!(m.subject.as_deref(), Some("user"));
        assert_eq!(m.access_count, 3);
        assert!((m.vitality - 0.7).abs() < 1e-9);
    }

    #[test]
    fn imports_minimal_schema_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("memory.db");
        make_source(&src, false);
        let target = MemoryDb::open_in_memory().unwrap();
        let report = import_remind_me_db(&target, &src).unwrap();
        assert_eq!(report.imported, 1);
        let hits = target.search("cats", 10).unwrap();
        assert_eq!(hits.len(), 1);
        // Optional columns absent → model defaults apply.
        assert_eq!(hits[0].memory_type, MemoryType::Unclassified);
        assert_eq!(hits[0].client, "unknown");
        assert!((hits[0].vitality - 1.0).abs() < 1e-9);
    }
}
