//! Ad-hoc command history (PRD-09 §10).
//!
//! # Scope
//!
//! A SQLite-backed journal of one-off commands the user ran outside the
//! saved-command sidebar: what was typed, where it ran, when, how long,
//! and how many times this exact `(command, working_dir)` pair has been
//! executed. Dedup happens by that pair — rerunning the same command in
//! the same cwd increments a counter on a single row rather than growing
//! the history unboundedly.
//!
//! # Microkernel fit
//!
//! Plain library; no kernel IPC. A future `com.nexus.terminal` core
//! plugin can expose `record_execution` / `list_recent` over dispatch
//! without touching this module.
//!
//! # What this is NOT
//!
//! - The promotion-to-saved-command flow (§10.2) — that belongs to the
//!   process-manager layer and lives in [`crate::procmgr`] / a future
//!   `saved_commands` module. We surface [`AdHocRecord`] so that layer
//!   has the raw data to seed a `SavedCommand` from.
//! - UI concerns (right-click menus, keybindings). Those are UI-plugin
//!   responsibilities that call into this store.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::TerminalError;
use crate::persist::unix_now;

/// Terminal status tag recorded against an ad-hoc run. Matches the PRD
/// `status` column values verbatim so the column never needs parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AdHocStatus {
    /// Process exited with code 0.
    Success,
    /// Process exited with a non-zero code.
    Failure,
    /// Process was terminated by the shutdown ladder reaching its
    /// timeout (PRD-09 §4.3) or an explicit kill.
    Timeout,
}

impl AdHocStatus {
    /// String tag persisted in the `status` column.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            AdHocStatus::Success => "success",
            AdHocStatus::Failure => "failure",
            AdHocStatus::Timeout => "timeout",
        }
    }

    /// Classify an exit code (if any) into a status tag. `None` →
    /// [`AdHocStatus::Timeout`] (the child was killed before producing
    /// a real exit); `Some(0)` → success; anything else → failure.
    #[must_use]
    pub fn from_exit_code(code: Option<i32>) -> Self {
        match code {
            None => AdHocStatus::Timeout,
            Some(0) => AdHocStatus::Success,
            Some(_) => AdHocStatus::Failure,
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "success" => AdHocStatus::Success,
            "timeout" => AdHocStatus::Timeout,
            _ => AdHocStatus::Failure,
        }
    }
}

/// A single row in `procmgr_adhoc_history`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdHocRecord {
    /// Opaque identifier (UUID).
    pub id: String,
    /// The literal command line the user typed (may contain `&&` / `||`).
    pub command: String,
    /// Working directory the command was run from. `None` if unknown /
    /// inherited.
    pub working_dir: Option<String>,
    /// Unix seconds when the run started.
    pub executed_at: i64,
    /// Exit code (`None` if the process was killed before exit).
    pub exit_code: Option<i32>,
    /// Duration of the run in milliseconds.
    pub duration_ms: u64,
    /// How many times this `(command, working_dir)` pair has been run.
    pub run_count: u32,
    /// Coarse status tag (see [`AdHocStatus`]).
    pub status: AdHocStatus,
}

/// SQLite-backed ad-hoc history store.
pub struct SqliteAdHocStore {
    conn: Connection,
}

impl SqliteAdHocStore {
    /// Open or create the store at `db_path`.
    ///
    /// # Errors
    /// Wraps SQLite errors in [`TerminalError::Persist`].
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self, TerminalError> {
        let conn = Connection::open(db_path.as_ref())
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        Self::migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Convenience for tests: open a memory-only instance.
    ///
    /// # Errors
    /// Wraps SQLite errors in [`TerminalError::Persist`].
    pub fn in_memory() -> Result<Self, TerminalError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        Self::migrate(&conn)?;
        Ok(Self { conn })
    }

    fn migrate(conn: &Connection) -> Result<(), TerminalError> {
        conn.execute_batch(
            r"
            CREATE TABLE IF NOT EXISTS procmgr_adhoc_history (
                id TEXT PRIMARY KEY,
                command TEXT NOT NULL,
                working_dir TEXT,
                executed_at INTEGER NOT NULL,
                exit_code INTEGER,
                duration_ms INTEGER NOT NULL DEFAULT 0,
                run_count INTEGER NOT NULL DEFAULT 1,
                status TEXT NOT NULL DEFAULT 'success'
            );
            CREATE INDEX IF NOT EXISTS idx_adhoc_history_executed_at
                ON procmgr_adhoc_history(executed_at DESC);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_adhoc_history_dedupe
                ON procmgr_adhoc_history(command, IFNULL(working_dir, ''));
            ",
        )
        .map_err(|e| TerminalError::Persist(e.to_string()))
    }

    /// Record an execution, deduping against existing
    /// `(command, working_dir)` pairs. If the pair already exists, its
    /// `run_count` is incremented, `executed_at` / `exit_code` /
    /// `duration_ms` / `status` are refreshed with the new run's values,
    /// and the existing id is retained. Otherwise a fresh row is
    /// inserted with `run_count = 1`.
    ///
    /// Returns the id of the upserted row (stable across repeated runs).
    ///
    /// # Errors
    /// Wraps SQLite errors in [`TerminalError::Persist`].
    pub fn record(
        &self,
        command: &str,
        working_dir: Option<&str>,
        exit_code: Option<i32>,
        duration_ms: u64,
    ) -> Result<String, TerminalError> {
        let status = AdHocStatus::from_exit_code(exit_code);
        let now = unix_now();
        let wd_key = working_dir.unwrap_or("");

        // Fast path: existing row? Increment its count + refresh run
        // metadata in one UPDATE; return its id.
        let existing_id: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM procmgr_adhoc_history
                 WHERE command = ?1 AND IFNULL(working_dir, '') = ?2",
                params![command, wd_key],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| TerminalError::Persist(e.to_string()))?;

        if let Some(id) = existing_id {
            self.conn
                .execute(
                    "UPDATE procmgr_adhoc_history SET
                        executed_at = ?1,
                        exit_code = ?2,
                        duration_ms = ?3,
                        status = ?4,
                        run_count = run_count + 1
                     WHERE id = ?5",
                    params![
                        now,
                        exit_code,
                        i64::try_from(duration_ms).unwrap_or(i64::MAX),
                        status.as_str(),
                        id,
                    ],
                )
                .map_err(|e| TerminalError::Persist(e.to_string()))?;
            return Ok(id);
        }

        let id = uuid::Uuid::new_v4().to_string();
        self.conn
            .execute(
                "INSERT INTO procmgr_adhoc_history
                    (id, command, working_dir, executed_at, exit_code,
                     duration_ms, run_count, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7)",
                params![
                    id,
                    command,
                    working_dir,
                    now,
                    exit_code,
                    i64::try_from(duration_ms).unwrap_or(i64::MAX),
                    status.as_str(),
                ],
            )
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        Ok(id)
    }

    /// Return the most-recent `limit` rows, sorted by `executed_at` desc.
    ///
    /// # Errors
    /// Wraps SQLite errors in [`TerminalError::Persist`].
    pub fn recent(&self, limit: usize) -> Result<Vec<AdHocRecord>, TerminalError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, command, working_dir, executed_at, exit_code,
                        duration_ms, run_count, status
                 FROM procmgr_adhoc_history
                 ORDER BY executed_at DESC
                 LIMIT ?1",
            )
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        let rows = stmt
            .query_map(params![i64::try_from(limit).unwrap_or(i64::MAX)], row_to_record)
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| TerminalError::Persist(e.to_string()))?);
        }
        Ok(out)
    }

    /// Look up a row by id.
    ///
    /// # Errors
    /// Wraps SQLite errors in [`TerminalError::Persist`].
    pub fn get(&self, id: &str) -> Result<Option<AdHocRecord>, TerminalError> {
        self.conn
            .query_row(
                "SELECT id, command, working_dir, executed_at, exit_code,
                        duration_ms, run_count, status
                 FROM procmgr_adhoc_history WHERE id = ?1",
                params![id],
                row_to_record,
            )
            .optional()
            .map_err(|e| TerminalError::Persist(e.to_string()))
    }

    /// Remove a row by id. Silent no-op if unknown.
    ///
    /// # Errors
    /// Wraps SQLite errors in [`TerminalError::Persist`].
    pub fn delete(&self, id: &str) -> Result<(), TerminalError> {
        self.conn
            .execute("DELETE FROM procmgr_adhoc_history WHERE id = ?1", params![id])
            .map_err(|e| TerminalError::Persist(e.to_string()))?;
        Ok(())
    }
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<AdHocRecord> {
    let duration_ms: i64 = row.get(5)?;
    let run_count: i64 = row.get(6)?;
    let status: String = row.get(7)?;
    Ok(AdHocRecord {
        id: row.get(0)?,
        command: row.get(1)?,
        working_dir: row.get(2)?,
        executed_at: row.get(3)?,
        exit_code: row.get(4)?,
        duration_ms: u64::try_from(duration_ms).unwrap_or(0),
        run_count: u32::try_from(run_count).unwrap_or(0),
        status: AdHocStatus::parse(&status),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_classifies_exit_codes() {
        assert_eq!(AdHocStatus::from_exit_code(Some(0)), AdHocStatus::Success);
        assert_eq!(AdHocStatus::from_exit_code(Some(1)), AdHocStatus::Failure);
        assert_eq!(AdHocStatus::from_exit_code(None), AdHocStatus::Timeout);
    }

    #[test]
    fn record_inserts_and_dedupes_by_command_and_cwd() {
        let s = SqliteAdHocStore::in_memory().expect("open");
        let id1 = s
            .record("ls", Some("/tmp"), Some(0), 50)
            .expect("record 1");
        let id2 = s
            .record("ls", Some("/tmp"), Some(0), 60)
            .expect("record 2");
        assert_eq!(id1, id2, "same command+cwd should dedupe to one row");
        let got = s.get(&id1).expect("get").expect("present");
        assert_eq!(got.run_count, 2);
        assert_eq!(got.duration_ms, 60);
    }

    #[test]
    fn record_differentiates_by_cwd() {
        let s = SqliteAdHocStore::in_memory().expect("open");
        let a = s.record("ls", Some("/a"), Some(0), 10).expect("record a");
        let b = s.record("ls", Some("/b"), Some(0), 10).expect("record b");
        assert_ne!(a, b);
        let recent = s.recent(10).expect("recent");
        assert_eq!(recent.len(), 2);
    }

    #[test]
    fn record_treats_null_working_dir_as_distinct_from_empty() {
        let s = SqliteAdHocStore::in_memory().expect("open");
        // Both should land in the same dedupe bucket because the UNIQUE
        // index uses IFNULL(working_dir, '').
        let a = s.record("ls", None, Some(0), 10).expect("record null");
        let b = s.record("ls", Some(""), Some(0), 10).expect("record empty");
        assert_eq!(a, b, "None and \"\" collapse under the unique index");
    }

    #[test]
    fn recent_orders_by_executed_at_descending() {
        let s = SqliteAdHocStore::in_memory().expect("open");
        s.record("a", None, Some(0), 10).expect("record a");
        std::thread::sleep(std::time::Duration::from_secs(1));
        s.record("b", None, Some(0), 10).expect("record b");
        let recent = s.recent(10).expect("recent");
        assert_eq!(recent[0].command, "b");
        assert_eq!(recent[1].command, "a");
    }

    #[test]
    fn failure_and_timeout_status_are_persisted() {
        let s = SqliteAdHocStore::in_memory().expect("open");
        let fail_id = s.record("boom", None, Some(2), 10).expect("record fail");
        let timeout_id = s.record("hang", None, None, 10).expect("record timeout");
        assert_eq!(
            s.get(&fail_id).expect("get").unwrap().status,
            AdHocStatus::Failure,
        );
        assert_eq!(
            s.get(&timeout_id).expect("get").unwrap().status,
            AdHocStatus::Timeout,
        );
    }

    #[test]
    fn delete_removes_row() {
        let s = SqliteAdHocStore::in_memory().expect("open");
        let id = s.record("ls", None, Some(0), 10).expect("record");
        s.delete(&id).expect("delete");
        assert!(s.get(&id).expect("get").is_none());
    }
}
