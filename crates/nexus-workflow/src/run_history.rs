//! Persisted workflow run-history (BL-054 Phase 4 follow-up).
//!
//! The executor returns a [`crate::executor::WorkflowRun`] per
//! invocation but doesn't persist it — the BL-054 observability panel
//! needs durable "last run" / "outcome" state to fill in the
//! Automation tab's deferred columns. This module is the persistence
//! layer.
//!
//! # File shape
//!
//! `<forge>/.forge/workflows/run_history.json` is a single JSON
//! document — `{ "entries": [<RunHistoryEntry>, …] }` — capped at
//! [`RUN_HISTORY_CAP`] entries (newest first). Bounded so a forge
//! that runs a high-frequency cron workflow doesn't grow an
//! unbounded log; spillover is silently dropped from the tail.
//!
//! Failures (corrupt JSON, missing parent dir, write error) never
//! escalate — recording a run is best-effort and must not break the
//! executor that produced the row.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Default cap on the on-disk ring. Configurable per-store via
/// [`RunHistoryStore::with_cap`] for tests; production uses the
/// default. 200 ≈ 6 weeks of a daily cron, ample headroom for a
/// human-scale forge.
pub const RUN_HISTORY_CAP: usize = 200;

const HISTORY_FILENAME: &str = "run_history.json";

/// One persisted invocation.
///
/// All timestamps are RFC-3339 UTC. `finished_at` is `started_at`
/// when the run is recorded synchronously (no separate "still
/// running" state today; failures land here too once the executor
/// returns).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunHistoryEntry {
    /// `workflow.name` from the source document — the same key the
    /// `run` IPC accepts.
    pub workflow_name: String,
    /// RFC-3339 UTC timestamp the run started.
    pub started_at: String,
    /// RFC-3339 UTC timestamp the run finished (success or failure).
    pub finished_at: String,
    /// `true` when no step failed and the gate (if any) opened.
    pub success: bool,
    /// `true` when the workflow's `[condition]` evaluated to false
    /// and the executor short-circuited before running any step.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub condition_skipped: bool,
    /// Number of steps in the run record (zero for a
    /// condition-skipped run).
    #[serde(default)]
    pub step_count: u32,
    /// Free-form failure message when `success = false`. Comes from
    /// either the executor's per-step `Failed` outcome (first failure
    /// wins) or the dispatcher error if the run aborted before a
    /// step recorded an outcome.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// On-disk wrapper. Wrapping in a typed root means we can grow the
/// document (e.g. add a schema version) without breaking forward-
/// compat — a future reader sees the wrapper, ignores keys it
/// doesn't understand.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct OnDisk {
    #[serde(default)]
    entries: Vec<RunHistoryEntry>,
}

/// In-memory + on-disk store. Cheap to clone — wraps a `Mutex` so
/// the caller can share a handle across the executor and the IPC
/// dispatcher without an outer `Arc`.
#[derive(Debug)]
pub struct RunHistoryStore {
    path: PathBuf,
    cap: usize,
    inner: Mutex<Vec<RunHistoryEntry>>,
}

impl RunHistoryStore {
    /// Open (or initialise) the store at `<workflows_dir>/run_history.json`.
    /// Missing or corrupt files start empty — never fails.
    #[must_use]
    pub fn open(workflows_dir: &Path) -> Self {
        Self::with_cap(workflows_dir, RUN_HISTORY_CAP)
    }

    /// Like [`open`](Self::open) but with a caller-chosen cap. Used
    /// by tests to exercise the spillover path without writing 200+
    /// entries.
    #[must_use]
    pub fn with_cap(workflows_dir: &Path, cap: usize) -> Self {
        let path = workflows_dir.join(HISTORY_FILENAME);
        let entries = match std::fs::read_to_string(&path) {
            Ok(raw) => match serde_json::from_str::<OnDisk>(&raw) {
                Ok(doc) => doc.entries,
                Err(err) => {
                    tracing::warn!(
                        path = %path.display(),
                        err = %err,
                        "workflow run_history: parse failed; starting empty",
                    );
                    Vec::new()
                }
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(err) => {
                tracing::warn!(
                    path = %path.display(),
                    err = %err,
                    "workflow run_history: read failed; starting empty",
                );
                Vec::new()
            }
        };
        Self {
            path,
            cap,
            inner: Mutex::new(entries),
        }
    }

    /// Append a row. Newest-first ordering on disk; entries past
    /// [`Self::cap`] are dropped from the tail. Errors writing to
    /// disk are logged but never propagated — the executor must not
    /// break because the audit log can't be written.
    pub fn append(&self, entry: RunHistoryEntry) {
        let mut guard = match self.inner.lock() {
            Ok(g) => g,
            Err(poison) => {
                tracing::warn!("workflow run_history: mutex poisoned; recovering",);
                poison.into_inner()
            }
        };
        guard.insert(0, entry);
        if guard.len() > self.cap {
            guard.truncate(self.cap);
        }
        let snapshot = OnDisk {
            entries: guard.clone(),
        };
        drop(guard);
        if let Err(err) = self.write_to_disk(&snapshot) {
            tracing::warn!(
                path = %self.path.display(),
                err = %err,
                "workflow run_history: write failed; entry held in memory only",
            );
        }
    }

    /// Read the in-memory rows, optionally filtered by `name` and
    /// capped by `limit`. Returns a fresh `Vec` so the caller can
    /// serialize without holding the mutex.
    #[must_use]
    pub fn list(&self, name: Option<&str>, limit: Option<usize>) -> Vec<RunHistoryEntry> {
        let guard = match self.inner.lock() {
            Ok(g) => g,
            Err(poison) => poison.into_inner(),
        };
        let mut out: Vec<RunHistoryEntry> = guard
            .iter()
            .filter(|e| name.is_none_or(|n| e.workflow_name == n))
            .cloned()
            .collect();
        if let Some(n) = limit {
            if out.len() > n {
                out.truncate(n);
            }
        }
        out
    }

    fn write_to_disk(&self, doc: &OnDisk) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // serde_json::to_string_pretty is fine — the file is bounded
        // at RUN_HISTORY_CAP entries × ~200 bytes ≈ 40 KB worst case.
        let body = serde_json::to_string_pretty(doc)
            .map_err(|e| std::io::Error::other(format!("serialize run_history: {e}")))?;
        std::fs::write(&self.path, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn entry(name: &str, success: bool) -> RunHistoryEntry {
        RunHistoryEntry {
            workflow_name: name.to_string(),
            started_at: "2026-05-07T10:00:00Z".to_string(),
            finished_at: "2026-05-07T10:00:01Z".to_string(),
            success,
            condition_skipped: false,
            step_count: 1,
            error: if success {
                None
            } else {
                Some("boom".to_string())
            },
        }
    }

    #[test]
    fn open_missing_file_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let store = RunHistoryStore::open(tmp.path());
        assert!(store.list(None, None).is_empty());
    }

    #[test]
    fn append_persists_and_reads_back() {
        let tmp = TempDir::new().unwrap();
        let store = RunHistoryStore::open(tmp.path());
        store.append(entry("foo", true));
        store.append(entry("bar", false));
        // Reopen — the new store reads the file from disk.
        let reopened = RunHistoryStore::open(tmp.path());
        let rows = reopened.list(None, None);
        assert_eq!(rows.len(), 2);
        // Newest-first ordering.
        assert_eq!(rows[0].workflow_name, "bar");
        assert_eq!(rows[1].workflow_name, "foo");
    }

    #[test]
    fn list_filters_by_name() {
        let tmp = TempDir::new().unwrap();
        let store = RunHistoryStore::open(tmp.path());
        store.append(entry("foo", true));
        store.append(entry("bar", true));
        store.append(entry("foo", false));
        let foos = store.list(Some("foo"), None);
        assert_eq!(foos.len(), 2);
        assert!(foos.iter().all(|e| e.workflow_name == "foo"));
    }

    #[test]
    fn list_caps_at_limit() {
        let tmp = TempDir::new().unwrap();
        let store = RunHistoryStore::open(tmp.path());
        store.append(entry("a", true));
        store.append(entry("b", true));
        store.append(entry("c", true));
        let rows = store.list(None, Some(2));
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn cap_drops_oldest_entries() {
        let tmp = TempDir::new().unwrap();
        let store = RunHistoryStore::with_cap(tmp.path(), 2);
        store.append(entry("oldest", true));
        store.append(entry("middle", true));
        store.append(entry("newest", true));
        let rows = store.list(None, None);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].workflow_name, "newest");
        assert_eq!(rows[1].workflow_name, "middle");
    }

    #[test]
    fn corrupt_file_starts_empty_does_not_clobber_until_write() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("run_history.json");
        std::fs::write(&path, "{not json").unwrap();
        let store = RunHistoryStore::open(tmp.path());
        // In-memory: empty (corrupt parse falls back to []).
        assert!(store.list(None, None).is_empty());
        // The corrupt bytes are still on disk until we write — the
        // store does not eagerly clobber the user's file.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{not json");
        // First append rewrites the file with valid JSON.
        store.append(entry("first", true));
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"workflow_name\": \"first\""));
    }
}
