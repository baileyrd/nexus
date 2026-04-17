//! Session persistence — PRD-09 §2.2 / §2.3.
//!
//! # Scope
//!
//! A SQLite-backed store for session metadata plus on-disk scrollback
//! blobs, and the LRU eviction policy that backs the `max_sessions` cap
//! in [`crate::SessionManager`].
//!
//! # Microkernel fit
//!
//! This module is a plain library. The kernel event bus never reaches in;
//! a future `com.nexus.terminal` core plugin will own a
//! `Mutex<SessionManager>` + [`SqliteSessionStore`] and expose
//! `spawn` / `list` / `restore` over IPC. Keeping the persistence concerns
//! in this crate means no plugin boundary has to know about SQLite.
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
//! `{base_dir}/{session_id}/scrollback.bin`. SQLite holds only the path
//! and byte length — blobs stay out of the database to keep queries
//! cheap and avoid rewriting a 10 MiB BLOB on every update.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

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

/// SQLite-backed implementation of PRD-09 §2.2 session persistence. Owns
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
    /// SQLite errors in [`TerminalError::Persist`].
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
    /// Wraps SQLite errors in [`TerminalError::Persist`].
    pub fn in_memory(scrollback_dir: impl Into<PathBuf>) -> Result<Self, TerminalError> {
        let scrollback_dir = scrollback_dir.into();
        std::fs::create_dir_all(&scrollback_dir)?;
        let conn = Connection::open_in_memory()
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        Self::migrate(&conn)?;
        Ok(Self {
            conn,
            scrollback_dir,
        })
    }

    fn migrate(conn: &Connection) -> Result<(), TerminalError> {
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
            ",
        )
        .map_err(|e| TerminalError::Persist(e.to_string()))
    }

    /// Upsert a session's metadata. Subsequent saves with the same id
    /// overwrite the existing row.
    ///
    /// # Errors
    /// Wraps SQLite errors in [`TerminalError::Persist`].
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
    /// Wraps SQLite errors in [`TerminalError::Persist`].
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
    /// Wraps SQLite errors in [`TerminalError::Persist`].
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
    /// the id is unknown.
    ///
    /// # Errors
    /// Wraps SQLite errors in [`TerminalError::Persist`]; file errors
    /// in [`TerminalError::Io`].
    pub fn delete(&self, id: &str) -> Result<(), TerminalError> {
        self.conn
            .execute("DELETE FROM terminal_sessions WHERE id = ?1", params![id])
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
    /// # Errors
    /// Propagates [`TerminalError::Io`] on filesystem failures.
    pub fn save_scrollback(&self, id: &str, bytes: &[u8]) -> Result<(), TerminalError> {
        let path = self.scrollback_path(id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes)?;
        Ok(())
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
        let store = SqliteSessionStore::open(
            tmp.path().join("nexus.sqlite"),
            tmp.path().join("sessions"),
        )
        .expect("open");
        // Schema creation is idempotent.
        drop(store);
        let _store = SqliteSessionStore::open(
            tmp.path().join("nexus.sqlite"),
            tmp.path().join("sessions"),
        )
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
}
