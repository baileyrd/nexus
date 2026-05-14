//! BL-121 — FTS5-backed search over agent `history.jsonl` logs.
//!
//! Maps each [`crate::memory::MemoryEntry`] to a row in an FTS5
//! virtual table (`agent_history_fts`) keyed by `(agent_id,
//! entry_idx)`. The role is synthesised per variant — `UserGoal` /
//! `UserFeedback` → `"user"`, `AgentPlan` / `Decision` /
//! `StepExecution` → `"assistant"`, `ToolCall` → `"tool"`, `Error` →
//! `"error"`, `Artifact` → `"artifact"` — so callers can filter
//! chat-style ("show me every user goal mentioning 'rebase'") even
//! though the underlying log is shape-rich rather than role-tagged.
//!
//! The SQLite file lives at `<forge>/.forge/agent/transcripts.sqlite`
//! and is rebuildable from the on-disk JSONL logs ([`rebuild_from_disk`]).
//! Live indexing piggy-backs on
//! [`crate::memory::append_entry_to_path`]: the function consults
//! the process-global [`global`] handle and, if a store is wired,
//! upserts the entry it just appended.
//!
//! See PRD-14 §10's MCP shape for the wire-level reply (a
//! `Vec<TranscriptHit>` keyed by agent + entry index + role +
//! snippet).

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::memory::{normalize_agent_id, read_entries_from_path, MemoryEntry, AGENTS_DIR};

/// Forge-relative path to the FTS database.
pub const TRANSCRIPTS_DB_PATH: &str = ".forge/agent/transcripts.sqlite";

/// One match row returned by [`TranscriptStore::search`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct TranscriptHit {
    /// Agent id the entry came from. Matches
    /// `<forge>/.forge/agents/<agent_id>/`.
    pub agent_id: String,
    /// 0-based index of the entry inside the agent's `history.jsonl`.
    /// Stable across rebuilds because the JSONL is append-only.
    pub entry_idx: i64,
    /// Synthesised role label (`"user"` / `"assistant"` / `"tool"`
    /// / `"error"` / `"artifact"`).
    pub role: String,
    /// FTS5-generated snippet with `**…**` markers around hit
    /// tokens; safe to render verbatim in a chat-style UI.
    pub snippet: String,
    /// Full searched text — useful when the snippet truncates the
    /// match's surrounding context too aggressively for the caller.
    pub content: String,
    /// Unix epoch milliseconds the entry carried.
    pub ts_ms: i64,
    /// BM25 relevance score; lower is more relevant (SQLite FTS5
    /// convention). Negate for descending sort.
    pub score: f32,
}

/// Persistent FTS5 store. Cheap to clone (`Arc` over an internal
/// `Mutex<Connection>`); call [`open`] / [`open_in_memory`] once at
/// boot and stash the result in [`global`].
#[derive(Clone)]
pub struct TranscriptStore {
    inner: Arc<Mutex<Connection>>,
}

impl TranscriptStore {
    /// Open or create the FTS database at the given absolute path.
    /// Idempotent — subsequent calls reuse the file in place.
    ///
    /// # Errors
    /// Wraps SQLite errors in [`TranscriptError::Sqlite`].
    pub fn open(db_path: &Path) -> Result<Self, TranscriptError> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path).map_err(TranscriptError::Sqlite)?;
        Self::migrate(&conn)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(conn)),
        })
    }

    /// Open an in-memory store. Convenient for unit tests.
    ///
    /// # Errors
    /// Wraps SQLite errors.
    pub fn open_in_memory() -> Result<Self, TranscriptError> {
        let conn = Connection::open_in_memory().map_err(TranscriptError::Sqlite)?;
        Self::migrate(&conn)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(conn)),
        })
    }

    fn migrate(conn: &Connection) -> Result<(), TranscriptError> {
        // FTS5 virtual table. `content` is the only tokenised
        // column; `agent_id` / `entry_idx` / `role` / `ts_ms` are
        // `UNINDEXED` because we only filter on them, not search.
        // Excluded from any future backup export — fully
        // rebuildable from the on-disk JSONL logs.
        conn.execute_batch(
            r"CREATE VIRTUAL TABLE IF NOT EXISTS agent_history_fts USING fts5(
                agent_id UNINDEXED,
                entry_idx UNINDEXED,
                role UNINDEXED,
                content,
                ts_ms UNINDEXED,
                tokenize = 'porter'
            );",
        )
        .map_err(TranscriptError::Sqlite)?;
        Ok(())
    }

    /// `true` when the FTS table has at least one row. Used by
    /// [`initialize`] to decide whether to trigger a rebuild on
    /// first boot.
    ///
    /// # Errors
    /// Wraps SQLite errors.
    pub fn is_empty(&self) -> Result<bool, TranscriptError> {
        let g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let n: i64 = g
            .query_row("SELECT COUNT(*) FROM agent_history_fts", [], |r| r.get(0))
            .map_err(TranscriptError::Sqlite)?;
        Ok(n == 0)
    }

    /// Replace every row for `agent_id` with the supplied entries.
    /// `entry_idx` is the 0-based position inside the JSONL log —
    /// callers pass `entries.iter().enumerate()`.
    ///
    /// # Errors
    /// Wraps SQLite errors.
    pub fn replace_agent_history(
        &self,
        agent_id: &str,
        entries: &[MemoryEntry],
    ) -> Result<usize, TranscriptError> {
        let agent_id = normalize_agent_id(agent_id)
            .map_err(|_| TranscriptError::InvalidAgentId(agent_id.to_string()))?
            .to_string();
        let mut g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let tx = g.transaction().map_err(TranscriptError::Sqlite)?;
        tx.execute(
            "DELETE FROM agent_history_fts WHERE agent_id = ?1",
            params![&agent_id],
        )
        .map_err(TranscriptError::Sqlite)?;
        let mut count = 0usize;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO agent_history_fts (agent_id, entry_idx, role, content, ts_ms) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )
                .map_err(TranscriptError::Sqlite)?;
            for (idx, entry) in entries.iter().enumerate() {
                let (role, content) = render_entry(entry);
                if content.is_empty() {
                    continue;
                }
                stmt.execute(params![
                    agent_id,
                    i64::try_from(idx).unwrap_or(i64::MAX),
                    role,
                    content,
                    i64::try_from(entry.timestamp_ms()).unwrap_or(i64::MAX),
                ])
                .map_err(TranscriptError::Sqlite)?;
                count += 1;
            }
        }
        tx.commit().map_err(TranscriptError::Sqlite)?;
        Ok(count)
    }

    /// Append one entry to the index. Used by
    /// [`crate::memory::append_entry_to_path`] for live indexing.
    /// `entry_idx` should be the 0-based position the entry will
    /// occupy in the JSONL log (caller's responsibility to compute).
    ///
    /// # Errors
    /// Wraps SQLite errors.
    pub fn append_entry(
        &self,
        agent_id: &str,
        entry_idx: i64,
        entry: &MemoryEntry,
    ) -> Result<(), TranscriptError> {
        let agent_id = normalize_agent_id(agent_id)
            .map_err(|_| TranscriptError::InvalidAgentId(agent_id.to_string()))?
            .to_string();
        let (role, content) = render_entry(entry);
        if content.is_empty() {
            return Ok(());
        }
        let g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        g.execute(
            "INSERT INTO agent_history_fts (agent_id, entry_idx, role, content, ts_ms) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                agent_id,
                entry_idx,
                role,
                content,
                i64::try_from(entry.timestamp_ms()).unwrap_or(i64::MAX),
            ],
        )
        .map_err(TranscriptError::Sqlite)?;
        Ok(())
    }

    /// Search the FTS index. `agent_id` and `since_ts` filters AND-combine
    /// with the FTS5 MATCH expression. `query` is forwarded to FTS5
    /// verbatim — callers needing prefix matches must include the `*`.
    /// `limit` is clamped to `[1, 200]`.
    ///
    /// # Errors
    /// Wraps SQLite errors.
    pub fn search(&self, args: &SearchArgs) -> Result<Vec<TranscriptHit>, TranscriptError> {
        let limit = args.limit.unwrap_or(50).clamp(1, 200);
        let mut sql = String::from(
            "SELECT agent_id, entry_idx, role, \
                    snippet(agent_history_fts, 3, '**', '**', '…', 24) AS snip, \
                    content, ts_ms, bm25(agent_history_fts) AS score \
             FROM agent_history_fts \
             WHERE agent_history_fts MATCH ?1",
        );
        let mut sql_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        sql_params.push(Box::new(args.query.clone()));
        if let Some(aid) = args.agent_id.as_ref().filter(|s| !s.is_empty()) {
            sql.push_str(" AND agent_id = ?");
            sql_params.push(Box::new(aid.clone()));
        }
        if let Some(since) = args.since_ts_ms {
            sql.push_str(" AND ts_ms >= ?");
            sql_params.push(Box::new(since));
        }
        sql.push_str(" ORDER BY score ASC LIMIT ?");
        sql_params.push(Box::new(i64::from(limit)));

        let g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let mut stmt = g.prepare(&sql).map_err(TranscriptError::Sqlite)?;
        let refs: Vec<&dyn rusqlite::ToSql> = sql_params.iter().map(AsRef::as_ref).collect();
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(TranscriptHit {
                    agent_id: row.get(0)?,
                    entry_idx: row.get(1)?,
                    role: row.get(2)?,
                    snippet: row.get(3)?,
                    content: row.get(4)?,
                    ts_ms: row.get(5)?,
                    // BM25 scores live in single-digit range for our
                    // table sizes; the truncation is fine for a UI
                    // sort key.
                    #[allow(clippy::cast_possible_truncation)]
                    score: row.get::<_, f64>(6)? as f32,
                })
            })
            .map_err(TranscriptError::Sqlite)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(TranscriptError::Sqlite)?);
        }
        Ok(out)
    }
}

/// Arguments for [`TranscriptStore::search`] / the wire-level
/// `com.nexus.agent::search_transcripts` handler.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct SearchArgs {
    /// FTS5 MATCH query. Plain words AND-combine; quote phrases;
    /// trailing `*` enables prefix matching.
    pub query: String,
    /// Restrict to one agent id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Unix epoch ms floor — only entries at or after this time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_ts_ms: Option<i64>,
    /// Maximum hits to return. Clamped to `[1, 200]` server-side.
    /// Defaults to 50.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Errors raised by [`TranscriptStore`] operations.
#[derive(Debug, thiserror::Error)]
pub enum TranscriptError {
    /// Underlying SQLite failure.
    #[error("transcript-search sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// I/O failure (parent dir creation, JSONL read on rebuild).
    #[error("transcript-search io: {0}")]
    Io(#[from] std::io::Error),
    /// Agent id failed `normalize_agent_id`.
    #[error("transcript-search invalid agent id: {0}")]
    InvalidAgentId(String),
}

/// Synthesise a `(role, content)` pair for an entry. Empty content
/// means "skip this entry" (e.g. `ToolCall` records only success +
/// duration with no useful prose for search).
pub(crate) fn render_entry(entry: &MemoryEntry) -> (&'static str, String) {
    use std::fmt::Write as _;
    match entry {
        MemoryEntry::UserGoal { text, .. } => ("user", text.clone()),
        MemoryEntry::UserFeedback { text, .. } => ("user", text.clone()),
        MemoryEntry::AgentPlan {
            plan_id,
            step_count,
            ..
        } => (
            "assistant",
            format!("plan {plan_id} with {step_count} steps"),
        ),
        MemoryEntry::Decision {
            summary, rationale, ..
        } => {
            let mut out = String::new();
            out.push_str(summary);
            if !rationale.is_empty() {
                let _ = write!(out, " — {rationale}");
            }
            ("assistant", out)
        }
        MemoryEntry::StepExecution {
            step_id,
            success,
            summary,
            ..
        } => {
            let verdict = if *success { "ok" } else { "failed" };
            let mut out = format!("step {step_id} {verdict}");
            if !summary.is_empty() {
                let _ = write!(out, ": {summary}");
            }
            ("assistant", out)
        }
        MemoryEntry::ToolCall {
            tool, success, ..
        } => {
            let verdict = if *success { "ok" } else { "failed" };
            ("tool", format!("tool {tool} {verdict}"))
        }
        MemoryEntry::Error {
            message, step_id, ..
        } => {
            let mut out = message.clone();
            if let Some(sid) = step_id {
                let _ = write!(out, " (step {sid})");
            }
            ("error", out)
        }
        MemoryEntry::Artifact {
            path, description, ..
        } => {
            let mut out = format!("artifact {path}");
            if !description.is_empty() {
                let _ = write!(out, ": {description}");
            }
            ("artifact", out)
        }
        MemoryEntry::CompactedTurns {
            rounds_compressed,
            summary,
            ..
        } => {
            // BL-120 — compaction events are pure narrative; index
            // the summary text under the `assistant` role so a later
            // recall surface ("what did the agent decide?") still
            // matches against compressed history.
            (
                "assistant",
                format!("[compacted {rounds_compressed} rounds] {summary}"),
            )
        }
    }
}

/// Walk every `.forge/agents/<id>/history.jsonl` under `forge_root`
/// and replace each agent's rows wholesale. Used by [`initialize`]
/// to bootstrap an empty index from the JSONL ground truth, and by
/// operators who want to force a clean rebuild after a manual edit
/// to the JSONL.
///
/// Files that fail to read are skipped with a tracing warning so a
/// single corrupted log doesn't block the rebuild of every other
/// agent.
///
/// # Errors
/// Wraps SQLite errors on the index side; I/O errors at the
/// agent-dir level fall through to a tracing warn and skip.
pub fn rebuild_from_disk(
    forge_root: &Path,
    store: &TranscriptStore,
) -> Result<RebuildStats, TranscriptError> {
    let agents_dir = forge_root.join(AGENTS_DIR);
    let mut stats = RebuildStats::default();
    let entries_iter = match std::fs::read_dir(&agents_dir) {
        Ok(it) => it,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(stats),
        Err(err) => return Err(TranscriptError::Io(err)),
    };
    for dirent in entries_iter {
        let dirent = match dirent {
            Ok(d) => d,
            Err(err) => {
                tracing::warn!(%err, "BL-121: skipping unreadable entry under agents/");
                continue;
            }
        };
        if !dirent.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let agent_id = match dirent.file_name().into_string() {
            Ok(s) => s,
            Err(_) => continue,
        };
        if normalize_agent_id(&agent_id).is_err() {
            continue;
        }
        let history = dirent.path().join("history.jsonl");
        let entries = match read_entries_from_path(&history) {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!(agent_id = %agent_id, %err, "BL-121: skipping unreadable history.jsonl");
                continue;
            }
        };
        let inserted = store.replace_agent_history(&agent_id, &entries)?;
        stats.agents_indexed += 1;
        stats.entries_indexed += inserted;
    }
    Ok(stats)
}

/// Summary returned by [`rebuild_from_disk`] — used by tests +
/// operator-facing CLI in a future BL.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RebuildStats {
    /// Number of agent directories visited.
    pub agents_indexed: usize,
    /// Total entries inserted across every agent.
    pub entries_indexed: usize,
}

// ── Process-global handle ───────────────────────────────────────────────────

/// Singleton handle the agent core_plugin populates at `on_init`
/// and `memory::append_entry_to_path` consults for live indexing.
/// Returns `None` until [`initialize`] has set it.
#[must_use]
pub fn global() -> Option<TranscriptStore> {
    GLOBAL.get().cloned()
}

/// Set the process-global store. Idempotent: a second call after
/// success is a no-op, so a test that initialises an in-memory
/// store + a production boot that initialises the disk store can
/// coexist without contention.
pub fn initialize(forge_root: &Path) -> Result<TranscriptStore, TranscriptError> {
    if let Some(existing) = GLOBAL.get() {
        return Ok(existing.clone());
    }
    let store = TranscriptStore::open(&forge_root.join(TRANSCRIPTS_DB_PATH))?;
    if store.is_empty()? {
        match rebuild_from_disk(forge_root, &store) {
            Ok(stats) => tracing::debug!(
                ?stats,
                "BL-121: built initial transcript index from history.jsonl"
            ),
            Err(err) => tracing::warn!(%err, "BL-121: initial transcript rebuild failed"),
        }
    }
    let _ = GLOBAL.set(store.clone());
    Ok(store)
}

/// Test-only setter that injects a pre-built store into the global
/// slot. Used by integration tests to wire an in-memory store
/// without touching the filesystem.
#[doc(hidden)]
pub fn set_global_for_test(store: TranscriptStore) {
    let _ = GLOBAL.set(store);
}

static GLOBAL: OnceLock<TranscriptStore> = OnceLock::new();

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryEntry;

    fn now_ms() -> u64 {
        1_700_000_000_000
    }

    fn mk_store() -> TranscriptStore {
        TranscriptStore::open_in_memory().unwrap()
    }

    #[test]
    fn render_entry_synthesises_roles() {
        let (role, content) = render_entry(&MemoryEntry::UserGoal {
            text: "rebase the feature branch".into(),
            timestamp_ms: now_ms(),
        });
        assert_eq!(role, "user");
        assert!(content.contains("rebase"));

        let (role, content) = render_entry(&MemoryEntry::Decision {
            summary: "skip CI for docs-only".into(),
            rationale: "saves a minute per push".into(),
            timestamp_ms: now_ms(),
        });
        assert_eq!(role, "assistant");
        assert!(content.contains("skip CI"));
        assert!(content.contains("saves a minute"));

        let (role, content) = render_entry(&MemoryEntry::ToolCall {
            tool: "read_file".into(),
            success: false,
            duration_ms: 5,
            timestamp_ms: now_ms(),
        });
        assert_eq!(role, "tool");
        assert!(content.contains("failed"));

        let (role, _) = render_entry(&MemoryEntry::Artifact {
            path: "out/report.md".into(),
            description: "summary".into(),
            timestamp_ms: now_ms(),
        });
        assert_eq!(role, "artifact");
    }

    #[test]
    fn replace_and_search_round_trips() {
        let store = mk_store();
        let entries = vec![
            MemoryEntry::UserGoal {
                text: "investigate flaky rebase test".into(),
                timestamp_ms: now_ms(),
            },
            MemoryEntry::Decision {
                summary: "use git switch instead of checkout".into(),
                rationale: "checkout is being deprecated".into(),
                timestamp_ms: now_ms() + 1,
            },
        ];
        let n = store.replace_agent_history("coder", &entries).unwrap();
        assert_eq!(n, 2);

        let hits = store
            .search(&SearchArgs {
                query: "rebase".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].agent_id, "coder");
        assert_eq!(hits[0].role, "user");
        assert!(hits[0].snippet.contains("rebase"));
    }

    #[test]
    fn replace_overwrites_prior_rows() {
        let store = mk_store();
        store
            .replace_agent_history(
                "coder",
                &[MemoryEntry::UserGoal {
                    text: "alpha task".into(),
                    timestamp_ms: now_ms(),
                }],
            )
            .unwrap();
        store
            .replace_agent_history(
                "coder",
                &[MemoryEntry::UserGoal {
                    text: "beta task".into(),
                    timestamp_ms: now_ms(),
                }],
            )
            .unwrap();
        let alpha = store
            .search(&SearchArgs {
                query: "alpha".into(),
                ..Default::default()
            })
            .unwrap();
        let beta = store
            .search(&SearchArgs {
                query: "beta".into(),
                ..Default::default()
            })
            .unwrap();
        assert!(alpha.is_empty(), "stale row from first replace should be gone");
        assert_eq!(beta.len(), 1);
    }

    #[test]
    fn search_filters_by_agent_id() {
        let store = mk_store();
        store
            .replace_agent_history(
                "coder",
                &[MemoryEntry::UserGoal {
                    text: "shared word here".into(),
                    timestamp_ms: now_ms(),
                }],
            )
            .unwrap();
        store
            .replace_agent_history(
                "researcher",
                &[MemoryEntry::UserGoal {
                    text: "shared word elsewhere".into(),
                    timestamp_ms: now_ms(),
                }],
            )
            .unwrap();
        let scoped = store
            .search(&SearchArgs {
                query: "shared".into(),
                agent_id: Some("coder".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].agent_id, "coder");
    }

    #[test]
    fn search_filters_by_since_ts() {
        let store = mk_store();
        store
            .replace_agent_history(
                "coder",
                &[
                    MemoryEntry::UserGoal {
                        text: "early word".into(),
                        timestamp_ms: 100,
                    },
                    MemoryEntry::UserGoal {
                        text: "later word".into(),
                        timestamp_ms: 5_000,
                    },
                ],
            )
            .unwrap();
        let recent = store
            .search(&SearchArgs {
                query: "word".into(),
                since_ts_ms: Some(1_000),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(recent.len(), 1);
        assert!(recent[0].content.contains("later"));
    }

    #[test]
    fn search_limit_clamps_to_200_max() {
        let store = mk_store();
        let entries: Vec<MemoryEntry> = (0..10)
            .map(|i| MemoryEntry::UserGoal {
                text: format!("foo target {i}"),
                timestamp_ms: now_ms() + i as u64,
            })
            .collect();
        store.replace_agent_history("coder", &entries).unwrap();
        let huge = store
            .search(&SearchArgs {
                query: "target".into(),
                limit: Some(99_999),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(huge.len(), 10);
    }

    #[test]
    fn append_entry_lands_in_index() {
        let store = mk_store();
        let entry = MemoryEntry::Decision {
            summary: "use rebase not merge".into(),
            rationale: "linear history is easier to read".into(),
            timestamp_ms: now_ms(),
        };
        store.append_entry("coder", 0, &entry).unwrap();
        let hits = store
            .search(&SearchArgs {
                query: "linear".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entry_idx, 0);
    }

    #[test]
    fn is_empty_reports_table_state() {
        let store = mk_store();
        assert!(store.is_empty().unwrap());
        store
            .replace_agent_history(
                "coder",
                &[MemoryEntry::UserGoal {
                    text: "hi".into(),
                    timestamp_ms: now_ms(),
                }],
            )
            .unwrap();
        assert!(!store.is_empty().unwrap());
    }

    #[test]
    fn rebuild_from_disk_walks_every_agent() {
        // BL-121 — synthesise a forge with two agent dirs, run
        // rebuild, confirm stats match.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let agents = root.join(AGENTS_DIR);
        for (id, body) in &[
            ("coder", "first goal"),
            ("researcher", "second goal"),
        ] {
            let dir = agents.join(id);
            std::fs::create_dir_all(&dir).unwrap();
            let entry = MemoryEntry::UserGoal {
                text: (*body).to_string(),
                timestamp_ms: now_ms(),
            };
            let line = serde_json::to_vec(&entry).unwrap();
            let mut bytes = line;
            bytes.push(b'\n');
            std::fs::write(dir.join("history.jsonl"), bytes).unwrap();
        }
        let store = mk_store();
        let stats = rebuild_from_disk(root, &store).unwrap();
        assert_eq!(stats.agents_indexed, 2);
        assert_eq!(stats.entries_indexed, 2);
        let hits = store
            .search(&SearchArgs {
                query: "goal".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn rebuild_returns_zero_for_missing_agents_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let store = mk_store();
        let stats = rebuild_from_disk(tmp.path(), &store).unwrap();
        assert_eq!(stats, RebuildStats::default());
    }

    #[test]
    fn search_rejects_invalid_query_gracefully() {
        let store = mk_store();
        // FTS5 raises a SQLite error on syntactically-broken
        // queries (e.g. a bare quote). Caller should see a
        // wrapped Sqlite error rather than a panic.
        let err = store
            .search(&SearchArgs {
                query: "\"unterminated".into(),
                ..Default::default()
            })
            .unwrap_err();
        assert!(matches!(err, TranscriptError::Sqlite(_)));
    }
}
