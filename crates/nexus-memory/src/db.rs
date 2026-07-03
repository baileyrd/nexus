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

CREATE TABLE IF NOT EXISTS sync_state (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Phase 5 — durable backing for the three cognitive stores
-- (EpisodicStore / SemanticStore / ProceduralStore). Same database file
-- as the remind_me-parity `memories` table so `.forge/memory/` stays a
-- single-file store; the tables are disjoint and the in-memory facades
-- remain the read path.

CREATE TABLE IF NOT EXISTS episodic_log (
    seq         INTEGER PRIMARY KEY AUTOINCREMENT,
    id          TEXT NOT NULL UNIQUE,
    session_id  TEXT,
    kind        TEXT NOT NULL,
    content     TEXT NOT NULL DEFAULT '{}',
    occurred_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_episodic_session ON episodic_log(session_id);

CREATE TABLE IF NOT EXISTS semantic_facts (
    id             TEXT PRIMARY KEY,
    key            TEXT NOT NULL,
    content        TEXT NOT NULL,
    tags           TEXT NOT NULL DEFAULT '[]',
    source_session TEXT,
    stored_at      TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_semantic_key ON semantic_facts(key);

CREATE TABLE IF NOT EXISTS procedural_skills (
    id               TEXT PRIMARY KEY,
    name             TEXT NOT NULL,
    description      TEXT NOT NULL DEFAULT '',
    trigger_patterns TEXT NOT NULL DEFAULT '[]',
    template         TEXT NOT NULL DEFAULT '',
    source_session   TEXT,
    learned_at       TEXT NOT NULL,
    use_count        INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_procedural_name ON procedural_skills(name);
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

    /// Insert `m`, or overwrite an existing row with the same id **only if** `m`
    /// is newer (`updated_at` strictly greater) — last-write-wins. Returns
    /// `true` when a row was inserted or updated, `false` when an older/equal
    /// `m` was ignored. Used to apply records pulled from the sync hub.
    /// `created_at` is preserved on update (creation time is immutable).
    ///
    /// # Errors
    /// Returns an error on a write failure.
    pub fn upsert_lww(&self, m: &Memory) -> Result<bool> {
        let conn = self.pool.get()?;
        let n = conn.execute(
            "INSERT INTO memories (id, content, category, tags, source, metadata, \
                created_at, updated_at, client, node_id, capture_id, source_capture_id, \
                memory_type, status, superseded_by, subject, predicate, object, \
                accessed_at, access_count, decay_rate, vitality, base_weight) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23) \
             ON CONFLICT(id) DO UPDATE SET \
                content=excluded.content, category=excluded.category, tags=excluded.tags, \
                source=excluded.source, metadata=excluded.metadata, updated_at=excluded.updated_at, \
                client=excluded.client, node_id=excluded.node_id, capture_id=excluded.capture_id, \
                source_capture_id=excluded.source_capture_id, memory_type=excluded.memory_type, \
                status=excluded.status, superseded_by=excluded.superseded_by, subject=excluded.subject, \
                predicate=excluded.predicate, object=excluded.object, accessed_at=excluded.accessed_at, \
                access_count=excluded.access_count, decay_rate=excluded.decay_rate, \
                vitality=excluded.vitality, base_weight=excluded.base_weight \
             WHERE excluded.updated_at > memories.updated_at",
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
        Ok(n > 0)
    }

    /// Read a sync cursor/state value by key (`None` if unset).
    ///
    /// # Errors
    /// Returns an error on a query failure.
    pub fn sync_state_get(&self, key: &str) -> Result<Option<String>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare("SELECT value FROM sync_state WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    /// Write a sync cursor/state value (upserting by key).
    ///
    /// # Errors
    /// Returns an error on a write failure.
    pub fn sync_state_set(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO sync_state (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Fetch a memory by id. A tombstoned (`status = 'deleted'`, C36) row
    /// reads as absent, same as a row that was never inserted.
    ///
    /// # Errors
    /// Returns an error on a query or decode failure.
    pub fn get(&self, id: Uuid) -> Result<Option<Memory>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM memories m WHERE m.id = ?1 AND m.status != 'deleted'"
        ))?;
        let mut rows = stmt.query(params![id.to_string()])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_memory(row)?)),
            None => Ok(None),
        }
    }

    /// Fetch a memory by id **and record the access** — bumps `access_count`
    /// and sets `accessed_at` to now (the ACT-R vitality input). The bump is
    /// best-effort: a read-only or contended database still returns the memory,
    /// just without recording the access. Use [`get`](Self::get) for an
    /// internal load that must not count as a recall (e.g. read-modify-write).
    ///
    /// # Errors
    /// Returns an error on a query or decode failure of the fetch itself.
    pub fn get_recording_access(&self, id: Uuid) -> Result<Option<Memory>> {
        let Some(mut m) = self.get(id)? else {
            return Ok(None);
        };
        let now = Utc::now();
        // A recall must not fail because the access bump can't be written.
        if let Ok(conn) = self.pool.get() {
            if conn
                .execute(
                    "UPDATE memories SET access_count = access_count + 1, accessed_at = ?2 \
                     WHERE id = ?1",
                    params![id.to_string(), now.to_rfc3339()],
                )
                .is_ok()
            {
                m.access_count += 1;
                m.accessed_at = Some(now);
            }
        }
        Ok(Some(m))
    }

    /// List the most recently created memories, newest first.
    ///
    /// # Errors
    /// Returns an error on a query or decode failure.
    pub fn list(&self, limit: usize) -> Result<Vec<Memory>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM memories m ORDER BY m.created_at DESC LIMIT ?1"
        ))?;
        let out = stmt
            .query_map(params![clamp_limit(limit)], |row| {
                row_to_memory(row).map_err(|e| into_rusqlite(&e))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(out)
    }

    /// List memories strictly after the `(after_updated_at, after_id)` keyset
    /// cursor, ordered by `(updated_at, id)` — the push arm of sync. Pass
    /// `("1970-01-01T00:00:00+00:00", "")` to start from the beginning.
    ///
    /// # Errors
    /// Returns an error on a query or decode failure.
    pub fn list_since(
        &self,
        after_updated_at: &str,
        after_id: &str,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM memories m \
             WHERE m.updated_at > ?1 OR (m.updated_at = ?1 AND m.id > ?2) \
             ORDER BY m.updated_at ASC, m.id ASC LIMIT {}",
            clamp_limit(limit)
        ))?;
        let out = stmt
            .query_map(params![after_updated_at, after_id], |row| {
                row_to_memory(row).map_err(|e| into_rusqlite(&e))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(out)
    }

    /// All memories in a capture's lineage: the parent (`capture_id`) plus any
    /// decomposed children (`source_capture_id`), oldest first.
    ///
    /// # Errors
    /// Returns an error on a query or decode failure.
    pub fn list_by_capture(&self, capture_id: &str) -> Result<Vec<Memory>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM memories m \
             WHERE m.capture_id = ?1 OR m.source_capture_id = ?1 \
             ORDER BY m.created_at ASC, m.id ASC"
        ))?;
        let out = stmt
            .query_map(params![capture_id], |row| {
                row_to_memory(row).map_err(|e| into_rusqlite(&e))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(out)
    }

    /// List memories filtered by optional `category` / `memory_type` / `status`
    /// / `tag`, newest first. Filters are bound parameters (`None` means "any");
    /// the column names are fixed literals, so the dynamic `WHERE` is
    /// injection-safe. `tag` matches rows whose JSON `tags` array contains it.
    ///
    /// When `status` is `None` ("any status"), tombstoned rows
    /// (`status = 'deleted'`, C36) are still excluded — callers that want to
    /// see them must pass `status: Some("deleted")` explicitly, same as any
    /// other status value.
    ///
    /// # Errors
    /// Returns an error on a query or decode failure.
    pub fn list_filtered(
        &self,
        category: Option<&str>,
        memory_type: Option<&str>,
        status: Option<&str>,
        tag: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        let mut conds: Vec<String> = Vec::new();
        let mut vals: Vec<String> = Vec::new();
        for (col, val) in [
            ("m.category", category),
            ("m.memory_type", memory_type),
            ("m.status", status),
        ] {
            if let Some(v) = val {
                vals.push(v.to_string());
                conds.push(format!("{col} = ?{}", vals.len()));
            }
        }
        if status.is_none() {
            conds.push("m.status != 'deleted'".to_string());
        }
        if let Some(t) = tag {
            vals.push(t.to_string());
            conds.push(format!(
                "EXISTS (SELECT 1 FROM json_each(m.tags) je WHERE je.value = ?{})",
                vals.len()
            ));
        }
        let where_clause = if conds.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conds.join(" AND "))
        };
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM memories m {where_clause} ORDER BY m.created_at DESC LIMIT {}",
            clamp_limit(limit)
        ))?;
        let out = stmt
            .query_map(rusqlite::params_from_iter(vals.iter()), |row| {
                row_to_memory(row).map_err(|e| into_rusqlite(&e))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(out)
    }

    /// List SPO entity facts — rows whose `subject` is populated — filtered by
    /// optional `subject` / `predicate` / `object`, newest first. Filters are
    /// bound parameters (`None` means "any"); the column names are fixed
    /// literals, so the dynamic `WHERE` is injection-safe.
    ///
    /// # Errors
    /// Returns an error on a query or decode failure.
    pub fn list_facts(
        &self,
        subject: Option<&str>,
        predicate: Option<&str>,
        object: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        // A fact is any row with a subject; the optional equality filters narrow
        // it. Tombstoned rows (C36) are always excluded — facts have no
        // "show deleted" escape hatch, unlike list_filtered's explicit status arg.
        let mut conds: Vec<String> = vec![
            "m.subject IS NOT NULL".to_string(),
            "m.status != 'deleted'".to_string(),
        ];
        let mut vals: Vec<String> = Vec::new();
        for (col, val) in [
            ("m.subject", subject),
            ("m.predicate", predicate),
            ("m.object", object),
        ] {
            if let Some(v) = val {
                vals.push(v.to_string());
                conds.push(format!("{col} = ?{}", vals.len()));
            }
        }
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM memories m WHERE {} ORDER BY m.created_at DESC LIMIT {}",
            conds.join(" AND "),
            clamp_limit(limit)
        ))?;
        let out = stmt
            .query_map(rusqlite::params_from_iter(vals.iter()), |row| {
                row_to_memory(row).map_err(|e| into_rusqlite(&e))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(out)
    }

    /// Export every **non-tombstoned** (C36) stored memory as a full record,
    /// **oldest first** — the stable order suitable for a reproducible dump
    /// that can be fed back through an importer (round-trips the
    /// `remind_me`-parity schema). Forgotten memories stay forgotten in the
    /// export, matching the export's use as a backup/reimport source.
    ///
    /// # Errors
    /// Returns an error on a query or decode failure.
    pub fn export_all(&self) -> Result<Vec<Memory>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM memories m WHERE m.status != 'deleted' \
             ORDER BY m.created_at ASC, m.id ASC"
        ))?;
        let out = stmt
            .query_map([], |row| row_to_memory(row).map_err(|e| into_rusqlite(&e)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(out)
    }

    /// Distinct entities mentioned by SPO facts — every non-null `subject` and
    /// `object` on a non-tombstoned (C36) row — with the number of facts that
    /// mention each, most-frequent first (ties broken alphabetically). The
    /// `key` of each [`CategoryCount`] is the entity name.
    ///
    /// # Errors
    /// Returns an error on a query failure.
    pub fn list_entities(&self, limit: usize) -> Result<Vec<CategoryCount>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT name AS k, COUNT(*) AS c FROM (
                 SELECT subject AS name FROM memories WHERE subject IS NOT NULL AND status != 'deleted'
                 UNION ALL
                 SELECT object AS name FROM memories WHERE object IS NOT NULL AND status != 'deleted'
             ) GROUP BY name ORDER BY c DESC, k ASC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![clamp_limit(limit)], |r| {
                let count: i64 = r.get("c")?;
                Ok(CategoryCount {
                    key: r.get("k")?,
                    count: u64::try_from(count).unwrap_or(0),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Distinct tags across all non-tombstoned (C36) memories, with the
    /// number of memories carrying each, most-frequent first (ties
    /// alphabetical). The `key` of each [`CategoryCount`] is the tag.
    ///
    /// # Errors
    /// Returns an error on a query failure.
    pub fn list_tags(&self, limit: usize) -> Result<Vec<CategoryCount>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT je.value AS k, COUNT(*) AS c FROM memories m, json_each(m.tags) je \
             WHERE m.status != 'deleted' GROUP BY je.value ORDER BY c DESC, k ASC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![clamp_limit(limit)], |r| {
                let count: i64 = r.get("c")?;
                Ok(CategoryCount {
                    key: r.get("k")?,
                    count: u64::try_from(count).unwrap_or(0),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Full-text search over content/category/tags, best matches first.
    /// Tombstoning (C36) is a status flip, not a SQL `DELETE`, so a deleted
    /// row's content is still sitting in the external-content FTS index
    /// (the `memories_ad` delete trigger never fires); the join back to
    /// `memories` is what actually excludes it here.
    ///
    /// # Errors
    /// Returns an error on a query or decode failure.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Memory>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM memories m JOIN memories_fts f ON f.rowid = m.rowid \
             WHERE memories_fts MATCH ?1 AND m.status != 'deleted' ORDER BY rank LIMIT ?2"
        ))?;
        let out = stmt
            .query_map(params![query, clamp_limit(limit)], |row| {
                row_to_memory(row).map_err(|e| into_rusqlite(&e))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(out)
    }

    /// Active memories ranked by a freshly-computed [`compute_vitality`] score
    /// (highest first), top `limit`. Read-only: the score is computed on the
    /// fly and returned in each [`Memory::vitality`] field; the stored column
    /// is left untouched. v1 computes over all active rows in memory — fine for
    /// typical stores; a recency pre-filter can bound it later.
    ///
    /// # Errors
    /// Returns an error on a query or decode failure.
    pub fn vitality_report(&self, limit: usize) -> Result<Vec<Memory>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM memories m WHERE m.status = 'active'"
        ))?;
        let mut mems: Vec<Memory> = stmt
            .query_map([], |row| row_to_memory(row).map_err(|e| into_rusqlite(&e)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let now = Utc::now();
        for m in &mut mems {
            m.vitality = compute_vitality(m, now);
        }
        mems.sort_by(|a, b| b.vitality.total_cmp(&a.vitality));
        mems.truncate(limit);
        Ok(mems)
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

    /// Mark a memory superseded by `by`, stamping `superseded_by` and bumping
    /// `updated_at`. Returns `true` if a row changed. Used by `consolidate` to
    /// retire duplicates in favour of a canonical memory.
    ///
    /// # Errors
    /// Returns an error on a write failure.
    pub fn mark_superseded(&self, id: Uuid, by: Uuid) -> Result<bool> {
        let conn = self.pool.get()?;
        let n = conn.execute(
            "UPDATE memories SET status = 'superseded', superseded_by = ?2, updated_at = ?3 \
             WHERE id = ?1 AND status != 'superseded'",
            params![id.to_string(), by.to_string(), Utc::now().to_rfc3339()],
        )?;
        Ok(n > 0)
    }

    /// Tombstone a memory by id (C36, #389): sets `status = 'deleted'` and
    /// bumps `updated_at` rather than issuing a SQL `DELETE`. A hard delete
    /// is invisible to [`Self::list_since`] (the sync push scan), so a
    /// locally hard-deleted memory could never be observed by push and would
    /// silently resurrect from the hub or any peer that still had it. The
    /// row stays in the table — every normal read path
    /// (`get`/`list_filtered`/`search`/`list_facts`/`list_entities`/
    /// `export_all`/`stats`) excludes `status = 'deleted'` so it reads as
    /// gone — but push still scans it, sees the fresh `updated_at`, and
    /// forwards the tombstone.
    ///
    /// Returns `true` if a row was newly tombstoned (idempotent: deleting an
    /// already-deleted or nonexistent id returns `false`).
    ///
    /// # Errors
    /// Returns an error on a write failure.
    pub fn delete(&self, id: Uuid) -> Result<bool> {
        let conn = self.pool.get()?;
        let n = conn.execute(
            "UPDATE memories SET status = 'deleted', updated_at = ?2 \
             WHERE id = ?1 AND status != 'deleted'",
            params![id.to_string(), Utc::now().to_rfc3339()],
        )?;
        Ok(n > 0)
    }

    /// Total number of non-tombstoned (C36) stored memories.
    ///
    /// # Errors
    /// Returns an error on a query failure.
    pub fn count(&self) -> Result<u64> {
        let conn = self.pool.get()?;
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE status != 'deleted'",
            [],
            |r| r.get(0),
        )?;
        Ok(u64::try_from(n).unwrap_or(0))
    }

    /// Aggregate statistics over non-tombstoned (C36) memories: total count
    /// plus counts grouped by category, memory type, and source (each
    /// most-frequent first).
    ///
    /// # Errors
    /// Returns an error on a query failure.
    pub fn stats(&self) -> Result<MemoryStats> {
        let conn = self.pool.get()?;
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE status != 'deleted'",
            [],
            |r| r.get(0),
        )?;
        Ok(MemoryStats {
            count: u64::try_from(total).unwrap_or(0),
            by_category: grouped_count(&conn, "category")?,
            by_memory_type: grouped_count(&conn, "memory_type")?,
            by_source: grouped_count(&conn, "source")?,
        })
    }

    // ─── Cognitive stores (Phase 5) ────────────────────────────────────────
    //
    // Durable backing for `EpisodicStore` / `SemanticStore` /
    // `ProceduralStore`. The in-memory facades stay the read path; these
    // methods are their write-through + load-on-open counterparts.

    /// Append an episodic entry, pruning rows beyond `capacity` so the
    /// durable log mirrors the in-memory ring's bound. `capacity == 0`
    /// disables pruning (unbounded log).
    ///
    /// # Errors
    /// Returns an error if the row cannot be written.
    pub fn episodic_append(&self, entry: &crate::EpisodicEntry, capacity: usize) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT OR REPLACE INTO episodic_log (id, session_id, kind, content, occurred_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                entry.id.as_uuid().to_string(),
                entry.session_id.map(|s| s.to_string()),
                kind_to_str(entry.kind)?,
                entry.content.to_string(),
                entry.occurred_at.to_rfc3339(),
            ],
        )?;
        if capacity > 0 {
            conn.execute(
                "DELETE FROM episodic_log WHERE seq <= \
                     (SELECT MAX(seq) FROM episodic_log) - ?1",
                params![i64::try_from(capacity).unwrap_or(i64::MAX)],
            )?;
        }
        Ok(())
    }

    /// Most recent `limit` episodic entries in chronological order
    /// (oldest first) — the shape `EpisodicStore` loads its ring from.
    ///
    /// # Errors
    /// Returns an error on a query failure or an undecodable row.
    pub fn episodic_recent(&self, limit: usize) -> Result<Vec<crate::EpisodicEntry>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, kind, content, occurred_at FROM episodic_log \
             ORDER BY seq DESC LIMIT ?1",
        )?;
        let mut rows: Vec<crate::EpisodicEntry> = stmt
            .query_map(params![clamp_limit(limit)], |row| {
                row_to_episodic(row).map_err(|e| into_rusqlite(&e))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.reverse();
        Ok(rows)
    }

    /// Upsert a semantic fact, de-duplicating by `key` (last writer
    /// wins) — the same invariant `SemanticStore::store` maintains
    /// in memory.
    ///
    /// # Errors
    /// Returns an error if the rows cannot be written.
    pub fn semantic_upsert(&self, entry: &crate::SemanticEntry) -> Result<()> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM semantic_facts WHERE key = ?1",
            params![entry.key],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO semantic_facts \
                 (id, key, content, tags, source_session, stored_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                entry.id.as_uuid().to_string(),
                entry.key,
                entry.content,
                serde_json::to_string(&entry.tags).unwrap_or_else(|_| "[]".to_string()),
                entry.source_session.map(|s| s.to_string()),
                entry.stored_at.to_rfc3339(),
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Delete a semantic fact by id. Returns `true` when a row was removed.
    ///
    /// # Errors
    /// Returns an error if the delete cannot be executed.
    pub fn semantic_delete(&self, id: crate::SemanticId) -> Result<bool> {
        let conn = self.pool.get()?;
        let n = conn.execute(
            "DELETE FROM semantic_facts WHERE id = ?1",
            params![id.as_uuid().to_string()],
        )?;
        Ok(n > 0)
    }

    /// Every stored semantic fact — the load-on-open path for
    /// `SemanticStore`.
    ///
    /// # Errors
    /// Returns an error on a query failure or an undecodable row.
    pub fn semantic_all(&self) -> Result<Vec<crate::SemanticEntry>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT id, key, content, tags, source_session, stored_at FROM semantic_facts",
        )?;
        let rows = stmt
            .query_map([], |row| {
                row_to_semantic(row).map_err(|e| into_rusqlite(&e))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Upsert a procedural skill, de-duplicating by `name` (last writer
    /// wins) — the same invariant `ProceduralStore::register` maintains
    /// in memory.
    ///
    /// # Errors
    /// Returns an error if the rows cannot be written.
    pub fn procedural_upsert(&self, entry: &crate::ProceduralEntry) -> Result<()> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM procedural_skills WHERE name = ?1",
            params![entry.name],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO procedural_skills \
                 (id, name, description, trigger_patterns, template, \
                  source_session, learned_at, use_count) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                entry.id.as_uuid().to_string(),
                entry.name,
                entry.description,
                serde_json::to_string(&entry.trigger_patterns).unwrap_or_else(|_| "[]".to_string()),
                entry.template,
                entry.source_session.map(|s| s.to_string()),
                entry.learned_at.to_rfc3339(),
                i64::try_from(entry.use_count).unwrap_or(i64::MAX),
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Delete a procedural skill by id. Returns `true` when a row was
    /// removed.
    ///
    /// # Errors
    /// Returns an error if the delete cannot be executed.
    pub fn procedural_delete(&self, id: crate::ProceduralId) -> Result<bool> {
        let conn = self.pool.get()?;
        let n = conn.execute(
            "DELETE FROM procedural_skills WHERE id = ?1",
            params![id.as_uuid().to_string()],
        )?;
        Ok(n > 0)
    }

    /// Increment a skill's durable `use_count`. Returns `true` when the
    /// skill exists.
    ///
    /// # Errors
    /// Returns an error if the update cannot be executed.
    pub fn procedural_record_use(&self, id: crate::ProceduralId) -> Result<bool> {
        let conn = self.pool.get()?;
        let n = conn.execute(
            "UPDATE procedural_skills SET use_count = use_count + 1 WHERE id = ?1",
            params![id.as_uuid().to_string()],
        )?;
        Ok(n > 0)
    }

    /// Every stored procedural skill — the load-on-open path for
    /// `ProceduralStore`.
    ///
    /// # Errors
    /// Returns an error on a query failure or an undecodable row.
    pub fn procedural_all(&self) -> Result<Vec<crate::ProceduralEntry>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, description, trigger_patterns, template, \
                    source_session, learned_at, use_count \
             FROM procedural_skills",
        )?;
        let rows = stmt
            .query_map([], |row| {
                row_to_procedural(row).map_err(|e| into_rusqlite(&e))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

/// Count non-tombstoned (C36) rows grouped by a fixed column (`category` /
/// `memory_type` / `source`), most-frequent first. `column` is an internal
/// literal supplied by [`MemoryDb::stats`] — never user input — so
/// interpolating it is safe.
fn grouped_count(conn: &rusqlite::Connection, column: &str) -> Result<Vec<CategoryCount>> {
    let sql = format!(
        "SELECT {column} AS k, COUNT(*) AS c FROM memories WHERE status != 'deleted' \
         GROUP BY {column} ORDER BY c DESC, k ASC"
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

/// Heuristic vitality score (ACT-R-inspired): rewards frequent and recent
/// access, decaying with age at the memory's own `decay_rate`. This is a v1
/// proxy over the stored fields (access count + last access); full ACT-R
/// base-level activation over the whole access history can refine it later.
///
/// `base_weight * (1 + access_count) / (1 + decay_rate * age_days)`, where
/// `age_days` is measured from the last access (or creation if never accessed).
#[allow(clippy::cast_precision_loss)] // counts/durations are small; f64 is ample.
fn compute_vitality(m: &Memory, now: DateTime<Utc>) -> f64 {
    let last = m.accessed_at.unwrap_or(m.created_at);
    let age_days = (now - last).num_seconds().max(0) as f64 / 86_400.0;
    m.base_weight * (1.0 + m.access_count as f64) / (1.0 + m.decay_rate * age_days)
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

/// Serialize an [`crate::EpisodicKind`] to its serde `snake_case` string
/// (`user_message`, `tool_call`, …) for the `episodic_log.kind` column.
fn kind_to_str(kind: crate::EpisodicKind) -> Result<String> {
    match serde_json::to_value(kind) {
        Ok(serde_json::Value::String(s)) => Ok(s),
        other => Err(MemoryDbError::Decode(format!(
            "episodic kind serialized to non-string: {other:?}"
        ))),
    }
}

fn str_to_kind(s: &str) -> Result<crate::EpisodicKind> {
    serde_json::from_value(serde_json::Value::String(s.to_string()))
        .map_err(|e| MemoryDbError::Decode(format!("episodic kind {s:?}: {e}")))
}

fn parse_opt_uuid(s: Option<String>) -> Result<Option<Uuid>> {
    s.map(|v| parse_uuid(&v)).transpose()
}

fn row_to_episodic(row: &Row<'_>) -> Result<crate::EpisodicEntry> {
    let id: String = row.get("id")?;
    let session_id: Option<String> = row.get("session_id")?;
    let kind: String = row.get("kind")?;
    let content: String = row.get("content")?;
    let occurred_at: String = row.get("occurred_at")?;
    Ok(crate::EpisodicEntry {
        id: crate::EpisodicId::from_uuid(parse_uuid(&id)?),
        session_id: parse_opt_uuid(session_id)?,
        kind: str_to_kind(&kind)?,
        content: serde_json::from_str(&content)
            .map_err(|e| MemoryDbError::Decode(format!("episodic content: {e}")))?,
        occurred_at: parse_dt(&occurred_at)?,
    })
}

fn row_to_semantic(row: &Row<'_>) -> Result<crate::SemanticEntry> {
    let id: String = row.get("id")?;
    let tags: String = row.get("tags")?;
    let source_session: Option<String> = row.get("source_session")?;
    let stored_at: String = row.get("stored_at")?;
    Ok(crate::SemanticEntry {
        id: crate::SemanticId::from_uuid(parse_uuid(&id)?),
        key: row.get("key")?,
        content: row.get("content")?,
        tags: serde_json::from_str(&tags)
            .map_err(|e| MemoryDbError::Decode(format!("semantic tags: {e}")))?,
        source_session: parse_opt_uuid(source_session)?,
        stored_at: parse_dt(&stored_at)?,
    })
}

fn row_to_procedural(row: &Row<'_>) -> Result<crate::ProceduralEntry> {
    let id: String = row.get("id")?;
    let triggers: String = row.get("trigger_patterns")?;
    let source_session: Option<String> = row.get("source_session")?;
    let learned_at: String = row.get("learned_at")?;
    let use_count: i64 = row.get("use_count")?;
    Ok(crate::ProceduralEntry {
        id: crate::ProceduralId::from_uuid(parse_uuid(&id)?),
        name: row.get("name")?,
        description: row.get("description")?,
        trigger_patterns: serde_json::from_str(&triggers)
            .map_err(|e| MemoryDbError::Decode(format!("procedural triggers: {e}")))?,
        template: row.get("template")?,
        source_session: parse_opt_uuid(source_session)?,
        learned_at: parse_dt(&learned_at)?,
        use_count: u64::try_from(use_count).unwrap_or(0),
    })
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
        db.insert(&Memory::new("the deployment runs on Kubernetes"))
            .unwrap();
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
    fn delete_tombstones_the_row_instead_of_hard_deleting_it() {
        // C36 (#389) — a hard DELETE is invisible to list_since (the sync
        // push scan), so a deletion could never propagate to the hub or
        // other peers; delete() must instead flip status to leave a
        // syncable tombstone that every normal read path still treats as gone.
        let db = MemoryDb::open_in_memory().unwrap();
        let m = Memory::new("ephemeral note about zebras");
        db.insert(&m).unwrap();

        assert!(db.delete(m.id).unwrap());

        // list_since (the push scan) still observes the row, carrying the
        // deleted status forward so the tombstone can propagate.
        let since = db.list_since("1970-01-01T00:00:00+00:00", "", 10).unwrap();
        assert_eq!(since.len(), 1);
        assert_eq!(since[0].id, m.id);
        assert_eq!(since[0].status, MemoryStatus::Deleted);

        // Explicitly asking for deleted status still surfaces it (a future
        // "trash" view could use this); "any status" (None) does not.
        assert_eq!(
            db.list_filtered(None, None, Some("deleted"), None, 10)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            db.list_filtered(None, None, None, None, 10).unwrap().len(),
            0
        );

        // Idempotent: deleting an already-tombstoned (or nonexistent) id is
        // a no-op that reports no change.
        assert!(!db.delete(m.id).unwrap());
        assert!(!db.delete(Uuid::now_v7()).unwrap());
    }

    #[test]
    fn upsert_lww_applies_a_tombstone_pulled_from_a_peer() {
        // C36 — a peer's tombstone (status: deleted, newer updated_at) must
        // win over our still-active local copy via the same LWW rule as any
        // other field, since pull applies every record through upsert_lww.
        let db = MemoryDb::open_in_memory().unwrap();
        let mut m = Memory::new("shared note");
        db.insert(&m).unwrap();
        assert!(db.get(m.id).unwrap().is_some());

        m.status = MemoryStatus::Deleted;
        m.updated_at = Utc::now() + chrono::Duration::seconds(1);
        assert!(db.upsert_lww(&m).unwrap());

        assert!(db.get(m.id).unwrap().is_none());
        let since = db.list_since("1970-01-01T00:00:00+00:00", "", 10).unwrap();
        assert_eq!(since[0].status, MemoryStatus::Deleted);
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
    fn list_filtered_applies_filters() {
        let db = MemoryDb::open_in_memory().unwrap();
        db.insert(
            &Memory::new("ops semantic")
                .with_category("ops")
                .with_type(MemoryType::Semantic)
                .with_tags(["infra", "k8s"]),
        )
        .unwrap();
        db.insert(
            &Memory::new("ops episodic")
                .with_category("ops")
                .with_type(MemoryType::Episodic),
        )
        .unwrap();
        db.insert(
            &Memory::new("prefs semantic")
                .with_category("prefs")
                .with_type(MemoryType::Semantic)
                .with_tags(["infra"]),
        )
        .unwrap();

        assert_eq!(
            db.list_filtered(None, None, None, None, 10).unwrap().len(),
            3
        );
        assert_eq!(
            db.list_filtered(Some("ops"), None, None, None, 10)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            db.list_filtered(None, Some("semantic"), None, None, 10)
                .unwrap()
                .len(),
            2
        );
        let combined = db
            .list_filtered(Some("ops"), Some("semantic"), None, None, 10)
            .unwrap();
        assert_eq!(combined.len(), 1);
        assert_eq!(combined[0].content, "ops semantic");
        assert_eq!(
            db.list_filtered(None, None, Some("active"), None, 10)
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            db.list_filtered(None, None, Some("archived"), None, 10)
                .unwrap()
                .len(),
            0
        );
        // Tag filter: "infra" tags two rows, "k8s" tags one; combined with a
        // category filter it narrows further.
        assert_eq!(
            db.list_filtered(None, None, None, Some("infra"), 10)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            db.list_filtered(None, None, None, Some("k8s"), 10)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            db.list_filtered(Some("prefs"), None, None, Some("infra"), 10)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            db.list_filtered(None, None, None, Some("absent"), 10)
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn list_tags_counts_distinct_tags() {
        let db = MemoryDb::open_in_memory().unwrap();
        db.insert(&Memory::new("a").with_tags(["infra", "k8s"]))
            .unwrap();
        db.insert(&Memory::new("b").with_tags(["infra"])).unwrap();
        db.insert(&Memory::new("c")).unwrap(); // untagged
        let tags = db.list_tags(10).unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].key, "infra"); // most-frequent first
        assert_eq!(tags[0].count, 2);
        assert_eq!(tags[1].key, "k8s");
        assert_eq!(tags[1].count, 1);
    }

    #[test]
    fn list_facts_returns_only_spo_rows_and_filters() {
        let db = MemoryDb::open_in_memory().unwrap();
        // A plain memory (no subject) — must never appear in fact queries.
        db.insert(&Memory::new("just a note")).unwrap();
        let mut fact_a = Memory::new("Ada writes Rust");
        fact_a.subject = Some("ada".to_string());
        fact_a.predicate = Some("writes".to_string());
        fact_a.object = Some("rust".to_string());
        db.insert(&fact_a).unwrap();
        let mut fact_b = Memory::new("Ada lives in London");
        fact_b.subject = Some("ada".to_string());
        fact_b.predicate = Some("lives_in".to_string());
        fact_b.object = Some("london".to_string());
        db.insert(&fact_b).unwrap();

        // No filter -> both facts, the plain note excluded.
        assert_eq!(db.list_facts(None, None, None, 10).unwrap().len(), 2);
        // Subject filter -> both Ada facts.
        assert_eq!(db.list_facts(Some("ada"), None, None, 10).unwrap().len(), 2);
        // Predicate filter -> one.
        assert_eq!(
            db.list_facts(None, Some("writes"), None, 10).unwrap().len(),
            1
        );
        // Combined subject + object -> one exact fact.
        let hit = db.list_facts(Some("ada"), None, Some("rust"), 10).unwrap();
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].content, "Ada writes Rust");
        // No match.
        assert_eq!(
            db.list_facts(Some("grace"), None, None, 10).unwrap().len(),
            0
        );
    }

    #[test]
    fn list_entities_counts_subjects_and_objects() {
        let db = MemoryDb::open_in_memory().unwrap();
        db.insert(&Memory::new("plain note")).unwrap(); // no entity
        for (s, p, o) in [
            ("ada", "writes", "rust"),
            ("ada", "lives_in", "london"),
            ("grace", "writes", "cobol"),
        ] {
            let mut m = Memory::new(format!("{s} {p} {o}"));
            m.subject = Some(s.to_string());
            m.predicate = Some(p.to_string());
            m.object = Some(o.to_string());
            db.insert(&m).unwrap();
        }
        let ents = db.list_entities(10).unwrap();
        // ada(2) + grace(1) + rust(1) + london(1) + cobol(1) = 5 distinct.
        assert_eq!(ents.len(), 5);
        // Most-frequent first: ada appears in two facts.
        assert_eq!(ents[0].key, "ada");
        assert_eq!(ents[0].count, 2);
        // The rest each appear once.
        assert!(ents[1..].iter().all(|e| e.count == 1));
        // limit is honoured.
        assert_eq!(db.list_entities(2).unwrap().len(), 2);
    }

    #[test]
    fn export_all_returns_every_row_oldest_first() {
        let db = MemoryDb::open_in_memory().unwrap();
        // Insert with explicit, out-of-order timestamps to pin the ordering.
        for (content, secs) in [("middle", 200), ("oldest", 100), ("newest", 300)] {
            let mut m = Memory::new(content);
            m.created_at = DateTime::from_timestamp(secs, 0).unwrap();
            db.insert(&m).unwrap();
        }
        let dump = db.export_all().unwrap();
        let order: Vec<&str> = dump.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(order, ["oldest", "middle", "newest"]);
    }

    #[test]
    fn get_recording_access_bumps_count_while_plain_get_does_not() {
        let db = MemoryDb::open_in_memory().unwrap();
        let m = Memory::new("recall me");
        db.insert(&m).unwrap();

        // Plain get never records an access.
        assert_eq!(db.get(m.id).unwrap().unwrap().access_count, 0);
        assert!(db.get(m.id).unwrap().unwrap().accessed_at.is_none());

        // Recording get bumps the count and stamps accessed_at each call.
        let first = db.get_recording_access(m.id).unwrap().unwrap();
        assert_eq!(first.access_count, 1);
        assert!(first.accessed_at.is_some());
        assert_eq!(
            db.get_recording_access(m.id).unwrap().unwrap().access_count,
            2
        );
        // The bump is durable, not just reflected in the returned struct.
        assert_eq!(db.get(m.id).unwrap().unwrap().access_count, 2);

        // Missing id stays None (no panic, no write).
        assert!(db.get_recording_access(Uuid::now_v7()).unwrap().is_none());
    }

    #[test]
    fn vitality_report_ranks_frequent_recent_first_and_excludes_archived() {
        let db = MemoryDb::open_in_memory().unwrap();
        // Stale: accessed long ago, never re-accessed.
        let mut stale = Memory::new("stale");
        stale.accessed_at = Some(Utc::now() - chrono::Duration::days(60));
        stale.access_count = 1;
        db.insert(&stale).unwrap();
        // Hot: accessed just now, many times.
        let mut hot = Memory::new("hot");
        hot.accessed_at = Some(Utc::now());
        hot.access_count = 20;
        db.insert(&hot).unwrap();
        // Archived: would score high but is excluded from the report.
        let mut archived = Memory::new("archived");
        archived.accessed_at = Some(Utc::now());
        archived.access_count = 99;
        archived.status = MemoryStatus::Archived;
        db.insert(&archived).unwrap();

        let report = db.vitality_report(10).unwrap();
        assert_eq!(report.len(), 2); // archived excluded
        assert_eq!(report[0].content, "hot"); // frequent + recent ranks first
        assert_eq!(report[1].content, "stale");
        assert!(report[0].vitality > report[1].vitality);
    }

    #[test]
    fn upsert_lww_inserts_then_applies_only_newer() {
        let db = MemoryDb::open_in_memory().unwrap();
        let mut m = Memory::new("v1");
        m.updated_at = DateTime::from_timestamp(1000, 0).unwrap();
        // First upsert inserts.
        assert!(db.upsert_lww(&m).unwrap());
        assert_eq!(db.get(m.id).unwrap().unwrap().content, "v1");

        // An older update is ignored (LWW).
        let mut older = m.clone();
        older.content = "stale".to_string();
        older.updated_at = DateTime::from_timestamp(500, 0).unwrap();
        assert!(!db.upsert_lww(&older).unwrap());
        assert_eq!(db.get(m.id).unwrap().unwrap().content, "v1");

        // A newer update wins.
        let mut newer = m.clone();
        newer.content = "v2".to_string();
        newer.updated_at = DateTime::from_timestamp(2000, 0).unwrap();
        assert!(db.upsert_lww(&newer).unwrap());
        assert_eq!(db.get(m.id).unwrap().unwrap().content, "v2");
        assert_eq!(db.count().unwrap(), 1);
        // FTS reflects the applied update.
        assert_eq!(db.search("v2", 5).unwrap().len(), 1);
        assert_eq!(db.search("v1", 5).unwrap().len(), 0);
    }

    #[test]
    fn list_since_keyset_walks_in_order() {
        let db = MemoryDb::open_in_memory().unwrap();
        for (content, secs) in [("a", 100), ("b", 200), ("c", 300)] {
            let mut m = Memory::new(content);
            m.created_at = DateTime::from_timestamp(secs, 0).unwrap();
            m.updated_at = DateTime::from_timestamp(secs, 0).unwrap();
            db.insert(&m).unwrap();
        }
        // From the beginning, page size 2 → a, b (oldest-updated first).
        let page1 = db.list_since("1970-01-01T00:00:00+00:00", "", 2).unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].content, "a");
        assert_eq!(page1[1].content, "b");
        // Resume after b → c.
        let last = &page1[1];
        let page2 = db
            .list_since(&last.updated_at.to_rfc3339(), &last.id.to_string(), 2)
            .unwrap();
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0].content, "c");
    }

    #[test]
    fn sync_state_round_trips() {
        let db = MemoryDb::open_in_memory().unwrap();
        assert!(db.sync_state_get("cursor").unwrap().is_none());
        db.sync_state_set("cursor", "2026-01-01T00:00:00+00:00|m1")
            .unwrap();
        assert_eq!(
            db.sync_state_get("cursor").unwrap().as_deref(),
            Some("2026-01-01T00:00:00+00:00|m1")
        );
        // Upsert overwrites.
        db.sync_state_set("cursor", "later").unwrap();
        assert_eq!(
            db.sync_state_get("cursor").unwrap().as_deref(),
            Some("later")
        );
    }

    #[test]
    fn mark_superseded_sets_status_and_pointer() {
        let db = MemoryDb::open_in_memory().unwrap();
        let loser = Memory::new("dup");
        let canonical = Memory::new("dup");
        db.insert(&loser).unwrap();
        db.insert(&canonical).unwrap();
        assert!(db.mark_superseded(loser.id, canonical.id).unwrap());
        let got = db.get(loser.id).unwrap().unwrap();
        assert_eq!(got.status, MemoryStatus::Superseded);
        assert_eq!(got.superseded_by, Some(canonical.id));
        // Idempotent: a second supersede is a no-op (already superseded).
        assert!(!db.mark_superseded(loser.id, canonical.id).unwrap());
        // Excluded from the active-only vitality report.
        assert!(db
            .vitality_report(10)
            .unwrap()
            .iter()
            .all(|m| m.id != loser.id));
    }

    #[test]
    fn list_by_capture_returns_parent_and_children() {
        let db = MemoryDb::open_in_memory().unwrap();
        let cap = "cap-123";
        // Parent capture.
        let mut parent = Memory::new("the whole turn");
        parent.capture_id = Some(cap.to_string());
        db.insert(&parent).unwrap();
        // Two decomposed children.
        for c in ["fact one", "fact two"] {
            let mut child = Memory::new(c);
            child.source_capture_id = Some(cap.to_string());
            db.insert(&child).unwrap();
        }
        // An unrelated memory.
        db.insert(&Memory::new("unrelated")).unwrap();

        let lineage = db.list_by_capture(cap).unwrap();
        assert_eq!(lineage.len(), 3);
        assert_eq!(lineage[0].content, "the whole turn"); // parent inserted first
        assert!(db.list_by_capture("nope").unwrap().is_empty());
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
