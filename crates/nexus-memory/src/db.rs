//! `SQLite` persistence for the memory engine.
//!
//! [`MemoryDb`] is the durable store beneath the in-memory facade. It owns the
//! `memories` table plus an external-content FTS5 index kept in sync by
//! triggers, mirroring the `remind_me` schema so its database imports 1:1.
//!
//! Vectors are intentionally **not** stored here — embeddings and vector search
//! reuse the existing `nexus-ai` vector store (design decision D-1), so there is
//! exactly one embedding path and no native `sqlite-vec` dependency.

use std::path::Path;

use chrono::{DateTime, Utc};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Row};
use uuid::Uuid;

use crate::model::{CategoryCount, Memory, MemoryStats, MemoryStatus, MemoryType};

/// Schema applied on open. Idempotent via `IF NOT EXISTS`. The full
/// `remind_me`-parity column set is created up front (columns are cheap); the
/// behaviour behind the later-phase columns (SPO facts, vitality) lands with
/// its feature.
const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS memories (
    id            TEXT PRIMARY KEY,
    content       TEXT NOT NULL,
    category      TEXT NOT NULL DEFAULT 'general',
    tags          TEXT NOT NULL DEFAULT '[]',
    source        TEXT NOT NULL DEFAULT 'manual',
    metadata      TEXT NOT NULL DEFAULT '{}',
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    client        TEXT NOT NULL DEFAULT 'unknown',
    node_id       TEXT,
    capture_id    TEXT,
    source_capture_id TEXT,
    memory_type   TEXT NOT NULL DEFAULT 'unclassified',
    status        TEXT NOT NULL DEFAULT 'active',
    superseded_by TEXT,
    subject       TEXT,
    predicate     TEXT,
    object        TEXT,
    accessed_at   TEXT,
    access_count  INTEGER NOT NULL DEFAULT 0,
    decay_rate    REAL NOT NULL DEFAULT 0.1,
    vitality      REAL NOT NULL DEFAULT 1.0,
    base_weight   REAL NOT NULL DEFAULT 1.0
);

CREATE TABLE IF NOT EXISTS chat_imports (
    import_id   TEXT PRIMARY KEY,
    filename    TEXT NOT NULL,
    hash        TEXT NOT NULL,
    imported_at TEXT NOT NULL,
    stats       TEXT NOT NULL DEFAULT '{}'
);

CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
    content, category, tags,
    content='memories', content_rowid='rowid'
);

CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
    INSERT INTO memories_fts(rowid, content, category, tags)
    VALUES (new.rowid, new.content, new.category, new.tags);
END;
CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, content, category, tags)
    VALUES ('delete', old.rowid, old.content, old.category, old.tags);
END;
CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, content, category, tags)
    VALUES ('delete', old.rowid, old.content, old.category, old.tags);
    INSERT INTO memories_fts(rowid, content, category, tags)
    VALUES (new.rowid, new.content, new.category, new.tags);
END;

CREATE INDEX IF NOT EXISTS idx_memories_category ON memories(category);
CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(memory_type);
CREATE INDEX IF NOT EXISTS idx_memories_status ON memories(status);
CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at);
";

/// Column list (table-qualified, in [`row_to_memory`] order) shared by all
/// row-returning queries. Qualified with `m.` so the FTS join is unambiguous.
const COLS: &str = "m.id, m.content, m.category, m.tags, m.source, m.metadata, \
m.created_at, m.updated_at, m.client, m.node_id, m.capture_id, m.source_capture_id, \
m.memory_type, m.status, m.superseded_by, m.subject, m.predicate, m.object, \
m.accessed_at, m.access_count, m.decay_rate, m.vitality, m.base_weight";

/// Errors from the memory database layer.
#[derive(Debug, thiserror::Error)]
pub enum MemoryDbError {
    /// Underlying `SQLite` error.
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Connection-pool error.
    #[error("pool: {0}")]
    Pool(#[from] r2d2::Error),
    /// Filesystem error (e.g. creating the database directory).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// A stored value could not be decoded into its model type.
    #[error("decode: {0}")]
    Decode(String),
}

/// Result alias for the memory database layer.
pub type Result<T> = std::result::Result<T, MemoryDbError>;

/// Durable, FTS-indexed store of [`Memory`] rows.
///
/// Cloneable and `Send`/`Sync` — clones share one connection pool, so all
/// workers in a runtime read and write the same database.
#[derive(Clone)]
pub struct MemoryDb {
    pool: Pool<SqliteConnectionManager>,
}

impl std::fmt::Debug for MemoryDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryDb").finish_non_exhaustive()
    }
}

/// Per-connection setup run on every pooled connection: WAL journal mode plus
/// a busy timeout, so the bus-capture pump and the IPC handlers can write the
/// same database file concurrently without hitting `database is locked`.
fn init_conn(conn: &mut rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch("PRAGMA journal_mode=WAL;\nPRAGMA busy_timeout=5000;")
}

impl MemoryDb {
    /// Open (creating if needed) a memory database at `path` and apply the schema.
    ///
    /// # Errors
    /// Returns an error if the connection pool cannot be built or the schema
    /// fails to apply.
    pub fn open(path: &Path) -> Result<Self> {
        let manager = SqliteConnectionManager::file(path).with_init(init_conn);
        let db = Self {
            pool: Pool::new(manager)?,
        };
        db.migrate()?;
        Ok(db)
    }

    /// Open an in-memory database backed by a single shared connection (tests).
    ///
    /// # Errors
    /// Returns an error if the connection pool cannot be built or the schema
    /// fails to apply.
    pub fn open_in_memory() -> Result<Self> {
        let manager = SqliteConnectionManager::memory().with_init(init_conn);
        let db = Self {
            pool: Pool::builder().max_size(1).build(manager)?,
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.pool.get()?.execute_batch(SCHEMA)?;
        Ok(())
    }

    /// Insert a memory and index it for full-text search.
    ///
    /// # Errors
    /// Returns an error if the row cannot be written.
    pub fn insert(&self, m: &Memory) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO memories (id, content, category, tags, source, metadata, \
                created_at, updated_at, client, node_id, capture_id, source_capture_id, \
                memory_type, status, superseded_by, subject, predicate, object, \
                accessed_at, access_count, decay_rate, vitality, base_weight) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)",
            params![
                m.id.to_string(),
                m.content,
                m.category,
                serde_json::to_string(&m.tags).unwrap_or_else(|_| "[]".to_string()),
                m.source,
                serde_json::to_string(&m.metadata).unwrap_or_else(|_| "{}".to_string()),
                m.created_at.to_rfc3339(),
                m.updated_at.to_rfc3339(),
                m.client,
                m.node_id,
                m.capture_id,
                m.source_capture_id,
                m.memory_type.as_str(),
                m.status.as_str(),
                m.superseded_by.map(|u| u.to_string()),
                m.subject,
                m.predicate,
                m.object,
                m.accessed_at.map(|t| t.to_rfc3339()),
                m.access_count,
                m.decay_rate,
                m.vitality,
                m.base_weight,
            ],
        )?;
        Ok(())
    }

    /// Fetch a memory by id.
    ///
    /// # Errors
    /// Returns an error on a query or decode failure.
    pub fn get(&self, id: Uuid) -> Result<Option<Memory>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM memories m WHERE m.id = ?1"))?;
        let mut rows = stmt.query(params![id.to_string()])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_memory(row)?)),
            None => Ok(None),
        }
    }

    /// List the most recently created memories, newest first.
    ///
    /// # Errors
    /// Returns an error on a query or decode failure.
    pub fn list(&self, limit: usize) -> Result<Vec<Memory>> {
        let conn = self.pool.get()?;
        let mut stmt =
            conn.prepare(&format!("SELECT {COLS} FROM memories m ORDER BY m.created_at DESC LIMIT ?1"))?;
        let out = stmt
            .query_map(params![clamp_limit(limit)], |row| {
                row_to_memory(row).map_err(|e| into_rusqlite(&e))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(out)
    }

    /// Full-text search over content/category/tags, best matches first.
    ///
    /// # Errors
    /// Returns an error on a query or decode failure.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Memory>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM memories m JOIN memories_fts f ON f.rowid = m.rowid \
             WHERE memories_fts MATCH ?1 ORDER BY rank LIMIT ?2"
        ))?;
        let out = stmt
            .query_map(params![query, clamp_limit(limit)], |row| {
                row_to_memory(row).map_err(|e| into_rusqlite(&e))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(out)
    }

    /// Replace a memory's mutable fields (matched by id), bumping `updated_at`
    /// to now. Returns `true` if a row was changed.
    ///
    /// # Errors
    /// Returns an error on a write failure.
    pub fn update(&self, m: &Memory) -> Result<bool> {
        let conn = self.pool.get()?;
        let n = conn.execute(
            "UPDATE memories SET content=?2, category=?3, tags=?4, source=?5, metadata=?6, \
                updated_at=?7, client=?8, memory_type=?9, status=?10, superseded_by=?11, \
                subject=?12, predicate=?13, object=?14 WHERE id=?1",
            params![
                m.id.to_string(),
                m.content,
                m.category,
                serde_json::to_string(&m.tags).unwrap_or_else(|_| "[]".to_string()),
                m.source,
                serde_json::to_string(&m.metadata).unwrap_or_else(|_| "{}".to_string()),
                Utc::now().to_rfc3339(),
                m.client,
                m.memory_type.as_str(),
                m.status.as_str(),
                m.superseded_by.map(|u| u.to_string()),
                m.subject,
                m.predicate,
                m.object,
            ],
        )?;
        Ok(n > 0)
    }

    /// Delete a memory by id. Returns `true` if a row was removed.
    ///
    /// # Errors
    /// Returns an error on a write failure.
    pub fn delete(&self, id: Uuid) -> Result<bool> {
        let conn = self.pool.get()?;
        let n = conn.execute("DELETE FROM memories WHERE id = ?1", params![id.to_string()])?;
        Ok(n > 0)
    }

    /// Total number of stored memories.
    ///
    /// # Errors
    /// Returns an error on a query failure.
    pub fn count(&self) -> Result<u64> {
        let conn = self.pool.get()?;
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))?;
        Ok(u64::try_from(n).unwrap_or(0))
    }

    /// Aggregate statistics: total count plus counts grouped by category,
    /// memory type, and source (each most-frequent first).
    ///
    /// # Errors
    /// Returns an error on a query failure.
    pub fn stats(&self) -> Result<MemoryStats> {
        let conn = self.pool.get()?;
        let total: i64 = conn.query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))?;
        Ok(MemoryStats {
            count: u64::try_from(total).unwrap_or(0),
            by_category: grouped_count(&conn, "category")?,
            by_memory_type: grouped_count(&conn, "memory_type")?,
            by_source: grouped_count(&conn, "source")?,
        })
    }
}

/// Count rows grouped by a fixed column (`category` / `memory_type` /
/// `source`), most-frequent first. `column` is an internal literal supplied by
/// [`MemoryDb::stats`] — never user input — so interpolating it is safe.
fn grouped_count(conn: &rusqlite::Connection, column: &str) -> Result<Vec<CategoryCount>> {
    let sql = format!(
        "SELECT {column} AS k, COUNT(*) AS c FROM memories GROUP BY {column} ORDER BY c DESC, k ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |r| {
            let count: i64 = r.get("c")?;
            Ok(CategoryCount {
                key: r.get("k")?,
                count: u64::try_from(count).unwrap_or(0),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn clamp_limit(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

/// Wrap a decode error as a `rusqlite::Error` so it can flow out of a
/// `query_map` closure (which must return `rusqlite::Result`).
fn into_rusqlite(e: &MemoryDbError) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        e.to_string(),
    )))
}

pub(crate) fn parse_uuid(s: &str) -> Result<Uuid> {
    Uuid::parse_str(s).map_err(|e| MemoryDbError::Decode(format!("uuid {s:?}: {e}")))
}

pub(crate) fn parse_dt(s: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| MemoryDbError::Decode(format!("timestamp {s:?}: {e}")))
}

fn row_to_memory(row: &Row<'_>) -> Result<Memory> {
    let tags_json: String = row.get("tags")?;
    let tags = serde_json::from_str::<Vec<String>>(&tags_json)
        .map_err(|e| MemoryDbError::Decode(format!("tags: {e}")))?;
    let metadata_json: String = row.get("metadata")?;
    let metadata = serde_json::from_str::<serde_json::Value>(&metadata_json)
        .map_err(|e| MemoryDbError::Decode(format!("metadata: {e}")))?;
    let superseded_by = match row.get::<_, Option<String>>("superseded_by")? {
        Some(s) => Some(parse_uuid(&s)?),
        None => None,
    };
    let accessed_at = match row.get::<_, Option<String>>("accessed_at")? {
        Some(s) => Some(parse_dt(&s)?),
        None => None,
    };
    Ok(Memory {
        id: parse_uuid(&row.get::<_, String>("id")?)?,
        content: row.get("content")?,
        category: row.get("category")?,
        tags,
        source: row.get("source")?,
        metadata,
        created_at: parse_dt(&row.get::<_, String>("created_at")?)?,
        updated_at: parse_dt(&row.get::<_, String>("updated_at")?)?,
        client: row.get("client")?,
        node_id: row.get("node_id")?,
        capture_id: row.get("capture_id")?,
        source_capture_id: row.get("source_capture_id")?,
        memory_type: MemoryType::from_db(&row.get::<_, String>("memory_type")?),
        status: MemoryStatus::from_db(&row.get::<_, String>("status")?),
        superseded_by,
        subject: row.get("subject")?,
        predicate: row.get("predicate")?,
        object: row.get("object")?,
        accessed_at,
        access_count: row.get("access_count")?,
        decay_rate: row.get("decay_rate")?,
        vitality: row.get("vitality")?,
        base_weight: row.get("base_weight")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Memory;

    #[test]
    fn insert_get_round_trip() {
        let db = MemoryDb::open_in_memory().unwrap();
        let m = Memory::new("Rust is the user's preferred language")
            .with_type(MemoryType::Semantic)
            .with_category("preferences")
            .with_client("claude")
            .with_tags(["lang", "pref"]);
        db.insert(&m).unwrap();
        let got = db.get(m.id).unwrap().expect("row present");
        assert_eq!(got.content, m.content);
        assert_eq!(got.memory_type, MemoryType::Semantic);
        assert_eq!(got.category, "preferences");
        assert_eq!(got.client, "claude");
        assert_eq!(got.tags, vec!["lang".to_string(), "pref".to_string()]);
    }

    #[test]
    fn fts_search_finds_by_content() {
        let db = MemoryDb::open_in_memory().unwrap();
        db.insert(&Memory::new("the deployment runs on Kubernetes")).unwrap();
        db.insert(&Memory::new("the cat sat on the mat")).unwrap();
        let hits = db.search("kubernetes", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].content.contains("Kubernetes"));
    }

    #[test]
    fn update_reindexes_fts() {
        let db = MemoryDb::open_in_memory().unwrap();
        let mut m = Memory::new("original text about apples");
        db.insert(&m).unwrap();
        m.content = "revised text about oranges".to_string();
        assert!(db.update(&m).unwrap());
        assert_eq!(db.search("apples", 10).unwrap().len(), 0);
        assert_eq!(db.search("oranges", 10).unwrap().len(), 1);
    }

    #[test]
    fn delete_removes_row_and_fts_entry() {
        let db = MemoryDb::open_in_memory().unwrap();
        let m = Memory::new("ephemeral note about zebras");
        db.insert(&m).unwrap();
        assert!(db.delete(m.id).unwrap());
        assert!(db.get(m.id).unwrap().is_none());
        assert_eq!(db.search("zebras", 10).unwrap().len(), 0);
        assert_eq!(db.count().unwrap(), 0);
    }

    #[test]
    fn list_orders_newest_first() {
        let db = MemoryDb::open_in_memory().unwrap();
        let mut older = Memory::new("first");
        older.created_at = Utc::now() - chrono::Duration::seconds(10);
        db.insert(&older).unwrap();
        db.insert(&Memory::new("second")).unwrap();
        let all = db.list(10).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].content, "second");
    }

    #[test]
    fn file_db_persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.db");
        let m = Memory::new("persisted across reopen");
        {
            let db = MemoryDb::open(&path).unwrap();
            db.insert(&m).unwrap();
        }
        let db2 = MemoryDb::open(&path).unwrap();
        assert_eq!(db2.count().unwrap(), 1);
        assert!(db2.get(m.id).unwrap().is_some());
    }
}
