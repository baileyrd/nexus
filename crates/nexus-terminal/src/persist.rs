//! Session persistence — PRD-09 §2.2 / §2.3.
//!
//! # Scope
//!
//! A `SQLite`-backed store for session metadata plus on-disk scrollback
//! blobs, and the LRU eviction policy that backs the `max_sessions` cap
//! in [`crate::SessionManager`].
//!
//! # Microkernel fit
//!
//! This module is a plain library. The kernel event bus never reaches in;
//! a future `com.nexus.terminal` core plugin will own a
//! `Mutex<SessionManager>` + [`SqliteSessionStore`] and expose
//! `spawn` / `list` / `restore` over IPC. Keeping the persistence concerns
//! in this crate means no plugin boundary has to know about `SQLite`.
//!
//! # Schema
//!
//! Mirrors the PRD's `terminal_sessions` table verbatim. We own migrations
//! internally — calling [`SqliteSessionStore::open`] on a fresh file or
//! an existing v1 file is idempotent.
//!
//! # Scrollback on disk
//!
//! Scrollback (§2.2) is stored as a raw-bytes blob at
//! `{base_dir}/{session_id}/scrollback.bin`. `SQLite` holds only the path
//! and byte length — blobs stay out of the database to keep queries
//! cheap and avoid rewriting a 10 MiB BLOB on every update.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::ansi::strip_ansi;
use crate::error::TerminalError;
use crate::session::SessionId;

/// Persisted session metadata — the PRD-09 §2.2 "session state
/// serialization" shape, plus enough bookkeeping for LRU eviction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// Stable id matching [`SessionId`].
    pub id: String,
    /// User-friendly label.
    pub name: String,
    /// URL-safe slug.
    pub slug: String,
    /// Absolute path of the spawned shell.
    pub shell: String,
    /// CWD the shell was spawned in.
    pub working_dir: Option<String>,
    /// Unix seconds at session creation.
    pub created_at: i64,
    /// Unix seconds of the last read/write/resize.
    pub last_accessed_at: i64,
    /// True if the PTY is currently alive. Persisted so restart can
    /// skip re-spawning detached sessions.
    pub is_active: bool,
    /// Ring buffer capacity at save time (bytes).
    pub buffer_size_bytes: u64,
}

impl SessionMetadata {
    /// Build a fresh metadata record with `created_at` / `last_accessed_at`
    /// set to now.
    #[must_use]
    pub fn new(
        id: &SessionId,
        name: impl Into<String>,
        slug: impl Into<String>,
        shell: impl Into<String>,
        working_dir: Option<String>,
        buffer_size_bytes: u64,
    ) -> Self {
        let now = unix_now();
        Self {
            id: id.as_str().to_string(),
            name: name.into(),
            slug: slug.into(),
            shell: shell.into(),
            working_dir,
            created_at: now,
            last_accessed_at: now,
            is_active: true,
            buffer_size_bytes,
        }
    }
}

/// BL-063 — one row from `cross_session_search`. Carries enough
/// context for the shell UI to jump to the originating session and
/// render a result list grouped by id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
pub struct ScrollbackHit {
    /// Session this row came from. Stable across the session's
    /// lifetime so the UI can map back to its label / metadata via
    /// `load_metadata`.
    pub session_id: String,
    /// Single line of ANSI-stripped text.
    pub text: String,
    /// Wall-clock millisecond when the scrollback was reindexed —
    /// every row from a single `save_scrollback` call shares the
    /// same `ts_ms`. Useful for `since_ts` filtering and for sorting
    /// hits newest-first in the UI.
    pub ts_ms: i64,
    /// 0-based line index within the source scrollback blob. Lets a
    /// "jump to line" affordance offset into the on-disk file
    /// without re-reading the FTS table.
    pub line_index: i64,
}

/// `SQLite`-backed implementation of PRD-09 §2.2 session persistence. Owns
/// the DB connection plus the scrollback base directory where blobs live.
pub struct SqliteSessionStore {
    conn: Connection,
    scrollback_dir: PathBuf,
}

impl SqliteSessionStore {
    /// Open or create the store at `db_path`. Scrollback blobs are
    /// written under `scrollback_dir/{session_id}/scrollback.bin`.
    ///
    /// # Errors
    /// Propagates [`TerminalError::Io`] on I/O failures and wraps
    /// `SQLite` errors in [`TerminalError::Persist`].
    pub fn open(
        db_path: impl AsRef<Path>,
        scrollback_dir: impl Into<PathBuf>,
    ) -> Result<Self, TerminalError> {
        let scrollback_dir = scrollback_dir.into();
        std::fs::create_dir_all(&scrollback_dir)?;
        let conn = Connection::open(db_path.as_ref())
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        Self::migrate(&conn)?;
        Ok(Self {
            conn,
            scrollback_dir,
        })
    }

    /// Open an in-memory store. Convenient for unit tests; data vanishes
    /// on drop.
    ///
    /// # Errors
    /// Wraps `SQLite` errors in [`TerminalError::Persist`].
    pub fn in_memory(scrollback_dir: impl Into<PathBuf>) -> Result<Self, TerminalError> {
        let scrollback_dir = scrollback_dir.into();
        std::fs::create_dir_all(&scrollback_dir)?;
        let conn =
            Connection::open_in_memory().map_err(|e| TerminalError::Persist(e.to_string()))?;
        Self::migrate(&conn)?;
        Ok(Self {
            conn,
            scrollback_dir,
        })
    }

    fn migrate(conn: &Connection) -> Result<(), TerminalError> {
        // BL-063 — `scrollback_fts` is an FTS5 virtual table over the
        // ANSI-stripped lines of every persisted scrollback blob. Only
        // `line_text` is searchable; `session_id` / `ts_ms` /
        // `line_index` are stored as `UNINDEXED` columns so SELECTs
        // can constrain results without paying tokenisation cost on
        // them. The `porter` tokenizer handles word-prefix queries
        // (`error*`) which the cross-session search UI relies on.
        //
        // Excluded from any future SQLite backup export — the FTS
        // table is rebuildable from the on-disk scrollback blobs and
        // doubles the export size if included.
        conn.execute_batch(
            r"
            CREATE TABLE IF NOT EXISTS terminal_sessions (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                slug TEXT UNIQUE,
                shell TEXT NOT NULL,
                working_dir TEXT,
                created_at INTEGER NOT NULL,
                last_accessed_at INTEGER NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 0,
                buffer_size_bytes INTEGER NOT NULL DEFAULT 10485760
            );
            CREATE INDEX IF NOT EXISTS idx_sessions_last_accessed
                ON terminal_sessions(last_accessed_at DESC);
            CREATE VIRTUAL TABLE IF NOT EXISTS scrollback_fts USING fts5(
                session_id UNINDEXED,
                line_text,
                ts_ms UNINDEXED,
                line_index UNINDEXED,
                tokenize = 'porter'
            );
            ",
        )
        .map_err(|e| TerminalError::Persist(e.to_string()))
    }

    /// Upsert a session's metadata. Subsequent saves with the same id
    /// overwrite the existing row.
    ///
    /// # Errors
    /// Wraps `SQLite` errors in [`TerminalError::Persist`].
    pub fn save_metadata(&self, meta: &SessionMetadata) -> Result<(), TerminalError> {
        self.conn
            .execute(
                "INSERT INTO terminal_sessions (
                    id, name, slug, shell, working_dir,
                    created_at, last_accessed_at, is_active, buffer_size_bytes
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    slug = excluded.slug,
                    shell = excluded.shell,
                    working_dir = excluded.working_dir,
                    last_accessed_at = excluded.last_accessed_at,
                    is_active = excluded.is_active,
                    buffer_size_bytes = excluded.buffer_size_bytes",
                params![
                    meta.id,
                    meta.name,
                    meta.slug,
                    meta.shell,
                    meta.working_dir,
                    meta.created_at,
                    meta.last_accessed_at,
                    i64::from(meta.is_active),
                    i64::try_from(meta.buffer_size_bytes).unwrap_or(i64::MAX),
                ],
            )
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        Ok(())
    }

    /// Load a metadata row by session id, or `None` if missing.
    ///
    /// # Errors
    /// Wraps `SQLite` errors in [`TerminalError::Persist`].
    pub fn load_metadata(&self, id: &str) -> Result<Option<SessionMetadata>, TerminalError> {
        self.conn
            .query_row(
                "SELECT id, name, slug, shell, working_dir,
                        created_at, last_accessed_at, is_active, buffer_size_bytes
                 FROM terminal_sessions WHERE id = ?1",
                params![id],
                row_to_meta,
            )
            .optional()
            .map_err(|e| TerminalError::Persist(e.to_string()))
    }

    /// List every persisted session, most recently accessed first.
    ///
    /// # Errors
    /// Wraps `SQLite` errors in [`TerminalError::Persist`].
    pub fn list_metadata(&self) -> Result<Vec<SessionMetadata>, TerminalError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, slug, shell, working_dir,
                        created_at, last_accessed_at, is_active, buffer_size_bytes
                 FROM terminal_sessions
                 ORDER BY last_accessed_at DESC",
            )
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        let rows = stmt
            .query_map([], row_to_meta)
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| TerminalError::Persist(e.to_string()))?);
        }
        Ok(out)
    }

    /// Delete a session row (and its scrollback file). Silent no-op if
    /// the id is unknown. BL-063 also drops every `scrollback_fts`
    /// row tagged with this id so cross-session search doesn't
    /// surface results from a session the user has explicitly
    /// removed.
    ///
    /// # Errors
    /// Wraps `SQLite` errors in [`TerminalError::Persist`]; file errors
    /// in [`TerminalError::Io`].
    pub fn delete(&self, id: &str) -> Result<(), TerminalError> {
        self.conn
            .execute("DELETE FROM terminal_sessions WHERE id = ?1", params![id])
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        self.conn
            .execute(
                "DELETE FROM scrollback_fts WHERE session_id = ?1",
                params![id],
            )
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        let path = self.scrollback_path(id);
        if path.exists() {
            // Best-effort: remove the file; surface I/O errors so the
            // caller sees failures to clean up.
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// Write `bytes` as the scrollback for `id`. Overwrites any prior
    /// scrollback; creates parent directories as needed.
    ///
    /// BL-063 — also (re)indexes the scrollback into the FTS5 virtual
    /// table. Each line is ANSI-stripped before insertion so the
    /// tokenizer doesn't see escape sequences. Whole-row replace
    /// (DELETE + bulk INSERT) per save: simpler than incremental
    /// updates, and `save_scrollback` is only called on session
    /// eviction (BL-062) — once per session lifetime — so the
    /// O(lines) cost amortises.
    ///
    /// # Errors
    /// Propagates [`TerminalError::Io`] on filesystem failures and
    /// [`TerminalError::Persist`] on FTS5 SQL errors.
    pub fn save_scrollback(&self, id: &str, bytes: &[u8]) -> Result<(), TerminalError> {
        let path = self.scrollback_path(id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes)?;
        self.reindex_scrollback(id, bytes)
    }

    /// BL-063 — rebuild the FTS index for `id` from a freshly-written
    /// byte snapshot. Splits on `\n`, strips ANSI per line, drops
    /// blank lines (they don't help search and triple the row
    /// count), and inserts into `scrollback_fts` inside a single
    /// transaction so the index is never seen half-populated by a
    /// concurrent reader. Wall-clock at the millisecond the call
    /// returns is recorded as `ts_ms` — we don't have a per-line
    /// timestamp on the on-disk blob, so all rows for a single
    /// reindex carry the same `ts_ms`.
    fn reindex_scrollback(&self, id: &str, bytes: &[u8]) -> Result<(), TerminalError> {
        let now_ms = unix_now_millis();
        // SAFETY-EQUIVALENT: `from_utf8_lossy` replaces invalid bytes
        // with U+FFFD so an ANSI-tinged blob with binary garbage
        // (curl progress bars, Ctrl-G, etc.) doesn't make the parse
        // panic. The lossy replacement is fine for FTS — the user
        // wouldn't search for the garbage byte anyway.
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        tx.execute(
            "DELETE FROM scrollback_fts WHERE session_id = ?1",
            params![id],
        )
        .map_err(|e| TerminalError::Persist(e.to_string()))?;
        // The on-disk blob is the raw byte stream including ANSI
        // sequences. We line-split first, then ANSI-strip each line,
        // because some sequences span newlines (cursor-positioning
        // codes) and stripping the entire blob risks merging adjacent
        // lines into one row.
        for (idx, raw_line) in bytes.split(|&b| b == b'\n').enumerate() {
            if raw_line.is_empty() {
                continue;
            }
            let stripped = strip_ansi(raw_line);
            let trimmed = stripped.trim_end_matches('\r');
            if trimmed.trim().is_empty() {
                continue;
            }
            tx.execute(
                "INSERT INTO scrollback_fts (session_id, line_text, ts_ms, line_index)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, trimmed, now_ms, i64::try_from(idx).unwrap_or(i64::MAX),],
            )
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        }
        tx.commit()
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        Ok(())
    }

    /// BL-063 — search every persisted session's scrollback for
    /// `query`. The structural shape mirrors the IPC handler
    /// (`com.nexus.terminal::cross_session_search`); the IPC layer
    /// is a thin pass-through so this is the load-bearing call
    /// site.
    ///
    /// # Args
    ///
    /// - `query`: FTS5 MATCH expression when `is_regex == false`, or
    ///   a regex pattern when `is_regex == true`.
    /// - `is_regex`: when `true`, ignores FTS5 prefiltering and runs
    ///   `regex_lite::Regex` over every indexed line (filtered by
    ///   `session_ids` / `since_ts`). Linear scan is acceptable
    ///   because the user only reaches this path for non-trivial
    ///   regexes; the literal path takes the FTS5 fast path.
    /// - `session_ids`: when `Some`, restricts the search to those
    ///   ids. `None` (or empty slice) searches every session.
    /// - `since_ts`: when `Some`, drops rows whose `ts_ms < since_ts`.
    /// - `limit`: hard cap on returned hits. The handler defaults to
    ///   100 when the caller omits it.
    ///
    /// # Errors
    /// Wraps `SQLite` and regex compilation errors in
    /// [`TerminalError::Persist`].
    pub fn cross_session_search(
        &self,
        query: &str,
        is_regex: bool,
        session_ids: Option<&[String]>,
        since_ts: Option<i64>,
        limit: usize,
    ) -> Result<Vec<ScrollbackHit>, TerminalError> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let limit_i = i64::try_from(limit.max(1)).unwrap_or(i64::MAX);
        // Build the WHERE clause + binding plan. We keep the literal
        // and regex paths split because rusqlite's parameter binding
        // doesn't accept variable-length IN lists without explicit
        // placeholder rendering.
        if is_regex {
            self.cross_session_search_regex(query, session_ids, since_ts, limit_i)
        } else {
            self.cross_session_search_fts(query, session_ids, since_ts, limit_i)
        }
    }

    fn cross_session_search_fts(
        &self,
        query: &str,
        session_ids: Option<&[String]>,
        since_ts: Option<i64>,
        limit: i64,
    ) -> Result<Vec<ScrollbackHit>, TerminalError> {
        let mut sql = String::from(
            "SELECT session_id, line_text, ts_ms, line_index
             FROM scrollback_fts
             WHERE scrollback_fts MATCH ?",
        );
        let mut binds: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(query.to_string())];
        push_session_filter(&mut sql, &mut binds, session_ids);
        if let Some(ts) = since_ts {
            sql.push_str(" AND ts_ms >= ?");
            binds.push(Box::new(ts));
        }
        sql.push_str(" ORDER BY ts_ms DESC, line_index ASC LIMIT ?");
        binds.push(Box::new(limit));
        let bound: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(bound.iter()), row_to_hit)
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| TerminalError::Persist(e.to_string()))?);
        }
        Ok(out)
    }

    fn cross_session_search_regex(
        &self,
        pattern: &str,
        session_ids: Option<&[String]>,
        since_ts: Option<i64>,
        limit: i64,
    ) -> Result<Vec<ScrollbackHit>, TerminalError> {
        let re = regex_lite::Regex::new(pattern)
            .map_err(|e| TerminalError::Persist(format!("invalid regex: {e}")))?;
        let mut sql = String::from(
            "SELECT session_id, line_text, ts_ms, line_index
             FROM scrollback_fts WHERE 1=1",
        );
        let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        push_session_filter(&mut sql, &mut binds, session_ids);
        if let Some(ts) = since_ts {
            sql.push_str(" AND ts_ms >= ?");
            binds.push(Box::new(ts));
        }
        sql.push_str(" ORDER BY ts_ms DESC, line_index ASC");
        let bound: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(bound.iter()), row_to_hit)
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            let hit = row.map_err(|e| TerminalError::Persist(e.to_string()))?;
            if re.is_match(&hit.text) {
                out.push(hit);
                if i64::try_from(out.len()).unwrap_or(i64::MAX) >= limit {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Read the scrollback bytes for `id`, or `None` if no scrollback
    /// file exists yet (fresh session, or a prior save never landed).
    ///
    /// # Errors
    /// Propagates [`TerminalError::Io`] on filesystem failures other
    /// than a missing file.
    pub fn load_scrollback(&self, id: &str) -> Result<Option<Vec<u8>>, TerminalError> {
        let path = self.scrollback_path(id);
        match std::fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Where `id`'s scrollback blob lives on disk.
    #[must_use]
    pub fn scrollback_path(&self, id: &str) -> PathBuf {
        self.scrollback_dir.join(id).join("scrollback.bin")
    }
}

fn row_to_meta(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionMetadata> {
    Ok(SessionMetadata {
        id: row.get(0)?,
        name: row.get(1)?,
        slug: row.get(2)?,
        shell: row.get(3)?,
        working_dir: row.get(4)?,
        created_at: row.get(5)?,
        last_accessed_at: row.get(6)?,
        is_active: row.get::<_, i64>(7)? != 0,
        buffer_size_bytes: {
            let v: i64 = row.get(8)?;
            u64::try_from(v).unwrap_or(0)
        },
    })
}

pub(crate) fn unix_now() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    )
    .unwrap_or(i64::MAX)
}

/// BL-063 — Unix-millis clock for FTS row timestamps. Kept private:
/// the precision matters for `since_ts` filtering inside a single
/// run, but external callers don't need millisecond accuracy.
fn unix_now_millis() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
    )
    .unwrap_or(i64::MAX)
}

/// BL-063 — splice a `session_id IN (...)` clause onto a search SQL
/// builder. Returns silently when `session_ids` is `None` or empty
/// so callers don't have to special-case "no filter".
fn push_session_filter(
    sql: &mut String,
    binds: &mut Vec<Box<dyn rusqlite::ToSql>>,
    session_ids: Option<&[String]>,
) {
    let Some(ids) = session_ids else { return };
    if ids.is_empty() {
        return;
    }
    sql.push_str(" AND session_id IN (");
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            sql.push(',');
        }
        sql.push('?');
        binds.push(Box::new(id.clone()));
    }
    sql.push(')');
}

fn row_to_hit(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScrollbackHit> {
    Ok(ScrollbackHit {
        session_id: row.get(0)?,
        text: row.get(1)?,
        ts_ms: row.get(2)?,
        line_index: row.get(3)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn meta(id: &str, name: &str, accessed: i64) -> SessionMetadata {
        SessionMetadata {
            id: id.into(),
            name: name.into(),
            slug: name.into(),
            shell: "/bin/sh".into(),
            working_dir: None,
            created_at: 1_000,
            last_accessed_at: accessed,
            is_active: true,
            buffer_size_bytes: 1024,
        }
    }

    #[test]
    fn open_creates_schema_and_scrollback_dir() {
        let tmp = tempdir().expect("tempdir");
        let store =
            SqliteSessionStore::open(tmp.path().join("nexus.sqlite"), tmp.path().join("sessions"))
                .expect("open");
        // Schema creation is idempotent.
        drop(store);
        let _store =
            SqliteSessionStore::open(tmp.path().join("nexus.sqlite"), tmp.path().join("sessions"))
                .expect("reopen");
    }

    #[test]
    fn save_load_and_list_metadata_roundtrips() {
        let tmp = tempdir().expect("tempdir");
        let store = SqliteSessionStore::in_memory(tmp.path()).expect("mem store");
        let a = meta("id-a", "build", 100);
        let b = meta("id-b", "test", 200);
        store.save_metadata(&a).expect("save a");
        store.save_metadata(&b).expect("save b");
        let loaded = store.load_metadata("id-a").expect("load").expect("present");
        assert_eq!(loaded, a);
        let list = store.list_metadata().expect("list");
        // Most recently accessed first.
        assert_eq!(list[0].id, "id-b");
        assert_eq!(list[1].id, "id-a");
    }

    #[test]
    fn save_metadata_upserts_on_same_id() {
        let tmp = tempdir().expect("tempdir");
        let store = SqliteSessionStore::in_memory(tmp.path()).expect("mem store");
        let mut m = meta("id-a", "old", 100);
        store.save_metadata(&m).expect("save old");
        m.name = "new".into();
        m.last_accessed_at = 500;
        store.save_metadata(&m).expect("save new");
        let loaded = store.load_metadata("id-a").expect("load").expect("present");
        assert_eq!(loaded.name, "new");
        assert_eq!(loaded.last_accessed_at, 500);
    }

    #[test]
    fn scrollback_roundtrip_and_delete_cleans_up() {
        let tmp = tempdir().expect("tempdir");
        let store = SqliteSessionStore::in_memory(tmp.path()).expect("mem store");
        let m = meta("id-a", "build", 100);
        store.save_metadata(&m).expect("save meta");
        store
            .save_scrollback("id-a", b"scroll-bytes")
            .expect("save scroll");
        let bytes = store
            .load_scrollback("id-a")
            .expect("load scroll")
            .expect("present");
        assert_eq!(bytes, b"scroll-bytes");
        store.delete("id-a").expect("delete");
        assert!(store.load_metadata("id-a").expect("load").is_none());
        assert!(store.load_scrollback("id-a").expect("load").is_none());
    }

    #[test]
    fn load_scrollback_returns_none_when_missing() {
        let tmp = tempdir().expect("tempdir");
        let store = SqliteSessionStore::in_memory(tmp.path()).expect("mem store");
        assert!(store.load_scrollback("ghost").expect("load").is_none());
    }

    // ── BL-063 — FTS5 indexing tests ────────────────────────────────

    #[test]
    fn save_scrollback_indexes_each_line_for_match() {
        let tmp = tempdir().expect("tempdir");
        let store = SqliteSessionStore::in_memory(tmp.path()).expect("mem store");
        store
            .save_scrollback(
                "id-a",
                b"build started\n\
                  error: could not compile\n\
                  hint: check feature flags\n",
            )
            .expect("save");
        let hits = store
            .cross_session_search("error", false, None, None, 50)
            .expect("search");
        assert_eq!(hits.len(), 1, "exactly one error line");
        assert!(hits[0].text.contains("could not compile"));
        assert_eq!(hits[0].session_id, "id-a");
    }

    #[test]
    fn save_scrollback_strips_ansi_before_indexing() {
        let tmp = tempdir().expect("tempdir");
        let store = SqliteSessionStore::in_memory(tmp.path()).expect("mem store");
        store
            .save_scrollback("id", b"\x1b[31mERROR\x1b[0m payload\n")
            .expect("save");
        // Searching by literal "ERROR" must match — the ANSI bytes
        // around it shouldn't have made it into the index.
        let hits = store
            .cross_session_search("ERROR", false, None, None, 50)
            .expect("search");
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].text.contains('\x1b'), "indexed text leaked ANSI");
    }

    #[test]
    fn cross_session_search_skips_blank_lines() {
        let tmp = tempdir().expect("tempdir");
        let store = SqliteSessionStore::in_memory(tmp.path()).expect("mem store");
        store
            .save_scrollback("id", b"alpha\n\n   \nbeta\n")
            .expect("save");
        // The blank lines must be omitted; both content lines are
        // indexed independently.
        let hits = store
            .cross_session_search("alpha OR beta", false, None, None, 50)
            .expect("search");
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn cross_session_search_constrains_to_session_ids_when_set() {
        let tmp = tempdir().expect("tempdir");
        let store = SqliteSessionStore::in_memory(tmp.path()).expect("mem store");
        store
            .save_scrollback("a", b"shared keyword\n")
            .expect("save a");
        store
            .save_scrollback("b", b"shared keyword\n")
            .expect("save b");
        let just_a = store
            .cross_session_search("keyword", false, Some(&["a".to_string()]), None, 50)
            .expect("search a");
        assert_eq!(just_a.len(), 1);
        assert_eq!(just_a[0].session_id, "a");
        let both = store
            .cross_session_search("keyword", false, None, None, 50)
            .expect("search both");
        assert_eq!(both.len(), 2);
    }

    #[test]
    fn cross_session_search_respects_since_ts() {
        let tmp = tempdir().expect("tempdir");
        let store = SqliteSessionStore::in_memory(tmp.path()).expect("mem store");
        store
            .save_scrollback("a", b"older line\n")
            .expect("save older");
        // Capture the wall-clock between the two saves; the second
        // save's ts_ms must be >= this anchor.
        std::thread::sleep(std::time::Duration::from_millis(2));
        let anchor = unix_now_millis();
        std::thread::sleep(std::time::Duration::from_millis(2));
        store
            .save_scrollback("b", b"newer line\n")
            .expect("save newer");
        let recent = store
            .cross_session_search("line", false, None, Some(anchor), 50)
            .expect("search since");
        assert_eq!(recent.len(), 1, "since_ts should drop the older save");
        assert_eq!(recent[0].session_id, "b");
    }

    #[test]
    fn cross_session_search_respects_limit() {
        let tmp = tempdir().expect("tempdir");
        let store = SqliteSessionStore::in_memory(tmp.path()).expect("mem store");
        store
            .save_scrollback("a", b"line one foo\nline two foo\nline three foo\n")
            .expect("save");
        let capped = store
            .cross_session_search("foo", false, None, None, 2)
            .expect("search");
        assert_eq!(capped.len(), 2);
    }

    #[test]
    fn cross_session_search_regex_path_filters_with_regex_lite() {
        let tmp = tempdir().expect("tempdir");
        let store = SqliteSessionStore::in_memory(tmp.path()).expect("mem store");
        store
            .save_scrollback(
                "id",
                b"port 3000 in use\n\
                  port 8080 in use\n\
                  unrelated noise\n",
            )
            .expect("save");
        let hits = store
            .cross_session_search(r"port \d{4}", true, None, None, 50)
            .expect("regex search");
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn cross_session_search_regex_invalid_pattern_surfaces_error() {
        let tmp = tempdir().expect("tempdir");
        let store = SqliteSessionStore::in_memory(tmp.path()).expect("mem store");
        store.save_scrollback("id", b"any\n").expect("save");
        let err = store
            .cross_session_search("(unclosed", true, None, None, 10)
            .unwrap_err();
        match err {
            TerminalError::Persist(msg) => assert!(msg.contains("invalid regex"), "got: {msg}"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn save_scrollback_replaces_existing_index_for_session() {
        let tmp = tempdir().expect("tempdir");
        let store = SqliteSessionStore::in_memory(tmp.path()).expect("mem store");
        store.save_scrollback("id", b"first run\n").expect("save 1");
        // Second save replaces the prior index — the old text must
        // disappear or cross-session search would return stale rows
        // forever.
        store
            .save_scrollback("id", b"second run\n")
            .expect("save 2");
        let still_first = store
            .cross_session_search("first", false, None, None, 50)
            .expect("search");
        assert!(still_first.is_empty(), "stale row from prior save");
        let now_second = store
            .cross_session_search("second", false, None, None, 50)
            .expect("search");
        assert_eq!(now_second.len(), 1);
    }

    #[test]
    fn delete_clears_fts_rows_for_session() {
        let tmp = tempdir().expect("tempdir");
        let store = SqliteSessionStore::in_memory(tmp.path()).expect("mem store");
        let m = meta("id", "build", 100);
        store.save_metadata(&m).expect("meta");
        store
            .save_scrollback("id", b"contained inside\n")
            .expect("save");
        store.delete("id").expect("delete");
        let hits = store
            .cross_session_search("contained", false, None, None, 50)
            .expect("search");
        assert!(hits.is_empty(), "FTS row should have been deleted");
    }

    #[test]
    fn empty_query_returns_no_hits() {
        let tmp = tempdir().expect("tempdir");
        let store = SqliteSessionStore::in_memory(tmp.path()).expect("mem store");
        store.save_scrollback("id", b"any line\n").expect("save");
        for q in ["", "   "] {
            let hits = store
                .cross_session_search(q, false, None, None, 50)
                .expect("search");
            assert!(hits.is_empty(), "empty query '{q:?}' should be a no-op");
        }
    }
}
