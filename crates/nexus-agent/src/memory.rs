//! Agent-scoped persistent memory — PRD-15 §5 (DG-33).
//!
//! Distinct from per-session transcripts (which live at
//! `<forge>/.forge/agent/sessions/<session_id>.json` and are owned by
//! the session loop). Memory is keyed by **agent id** rather than
//! session id, so a "coder" or "researcher" archetype accumulates a
//! continuity of decisions / artifacts across multiple invocations.
//!
//! Storage layout per PRD-15 §5:
//!
//! ```text
//! <forge>/.forge/agents/<agent_id>/
//!   history.jsonl       # append-only event log (one MemoryEntry per line)
//!   snapshots/          # dated MemorySnapshot rollups (optional)
//!   artifacts/          # generated files referenced by entries
//! ```
//!
//! The JSONL log + snapshots are rebuildable in spirit from the
//! per-session transcripts that already exist; agent memory is the
//! summary view the planner reads on the *next* invocation. The
//! filesystem layout is the source of record (file-as-truth
//! invariant — same as the rest of the forge).
//!
//! `MemoryStore::FileSystem` is the only backend shipped today.
//! `MemoryStore::Database` is the spec's other option; the surface
//! is set up so the IPC handlers can swap backends without
//! re-shaping callers.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

/// Where agent-scoped memory lives, relative to the forge root.
pub const AGENTS_DIR: &str = ".forge/agents";

/// Append-only event log file name, inside the agent's directory.
pub const HISTORY_FILE: &str = "history.jsonl";

/// Subdirectory for dated snapshots.
pub const SNAPSHOTS_DIR: &str = "snapshots";

/// Subdirectory for artifacts (generated files the agent produced).
pub const ARTIFACTS_DIR: &str = "artifacts";

/// One event in the agent's memory log. Matches PRD-15 §5's
/// `MemoryEntry` enum — variants cover the lifecycle of an agent
/// run plus user-facing feedback.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemoryEntry {
    /// Goal the user handed the agent.
    UserGoal {
        /// Goal text.
        text: String,
        /// Unix epoch milliseconds.
        timestamp_ms: u64,
    },
    /// Plan id the agent produced for this goal.
    AgentPlan {
        /// Plan identifier (UUID).
        plan_id: String,
        /// Number of steps in the plan.
        step_count: u32,
        /// Unix epoch milliseconds.
        timestamp_ms: u64,
    },
    /// One step's execution outcome.
    StepExecution {
        /// Step id within the plan.
        step_id: String,
        /// Whether the step succeeded.
        success: bool,
        /// Short summary string (truncated to avoid massive logs).
        summary: String,
        /// Unix epoch milliseconds.
        timestamp_ms: u64,
    },
    /// One tool call the agent dispatched.
    ToolCall {
        /// Tool name (matches `AgentToolSpec.name`).
        tool: String,
        /// Whether the call succeeded.
        success: bool,
        /// Wall-clock duration in milliseconds.
        duration_ms: u64,
        /// Unix epoch milliseconds.
        timestamp_ms: u64,
    },
    /// User-supplied feedback or correction.
    UserFeedback {
        /// Feedback text.
        text: String,
        /// Unix epoch milliseconds.
        timestamp_ms: u64,
    },
    /// Error encountered during the run.
    Error {
        /// Error message.
        message: String,
        /// Optional step id where the error originated.
        step_id: Option<String>,
        /// Unix epoch milliseconds.
        timestamp_ms: u64,
    },
    /// Decision the agent recorded (PRD-15 §5: "decisions retained
    /// indefinitely" — the prune policy must respect this).
    Decision {
        /// Short summary of the decision.
        summary: String,
        /// Free-form rationale text.
        rationale: String,
        /// Unix epoch milliseconds.
        timestamp_ms: u64,
    },
    /// Reference to a generated artifact (file on disk).
    Artifact {
        /// Forge-relative path (typically under
        /// `<agent_id>/artifacts/`).
        path: String,
        /// One-line description.
        description: String,
        /// Unix epoch milliseconds.
        timestamp_ms: u64,
    },
}

impl MemoryEntry {
    /// Unix-epoch timestamp the entry carries.
    #[must_use]
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            Self::UserGoal { timestamp_ms, .. }
            | Self::AgentPlan { timestamp_ms, .. }
            | Self::StepExecution { timestamp_ms, .. }
            | Self::ToolCall { timestamp_ms, .. }
            | Self::UserFeedback { timestamp_ms, .. }
            | Self::Error { timestamp_ms, .. }
            | Self::Decision { timestamp_ms, .. }
            | Self::Artifact { timestamp_ms, .. } => *timestamp_ms,
        }
    }

    /// Whether this entry is a `Decision` — these must survive
    /// `prune` per PRD-15 §5.
    #[must_use]
    pub fn is_decision(&self) -> bool {
        matches!(self, Self::Decision { .. })
    }
}

/// Errors the memory layer surfaces.
#[derive(Debug, Error)]
pub enum MemoryError {
    /// Filesystem I/O failure.
    #[error("I/O error at {path}: {source}")]
    Io {
        /// Path that failed.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// JSON serialization or parse failure.
    #[error("invalid memory entry JSON at {path}: {source}")]
    Json {
        /// Path that failed to parse.
        path: PathBuf,
        /// Underlying serde error.
        #[source]
        source: serde_json::Error,
    },
    /// Agent id couldn't be normalized to a safe slug.
    #[error("invalid agent id '{0}' — must be ASCII alphanumeric / `-` / `_`, 1..96 chars")]
    InvalidAgentId(String),
}

/// Validate and normalize an agent id into a filesystem-safe slug.
///
/// Matches the existing `history_path` slug rule used by
/// `core_plugin.rs` so memory paths follow the same trust posture.
///
/// # Errors
/// Returns [`MemoryError::InvalidAgentId`] when the id is empty,
/// over 96 chars, or contains characters outside
/// `[A-Za-z0-9_.\-]` (period is allowed so reverse-DNS ids like
/// `com.nexus.agent.coder` work).
pub fn normalize_agent_id(agent_id: &str) -> Result<&str, MemoryError> {
    if agent_id.is_empty() || agent_id.len() > 96 {
        return Err(MemoryError::InvalidAgentId(agent_id.to_string()));
    }
    let safe = agent_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    if !safe {
        return Err(MemoryError::InvalidAgentId(agent_id.to_string()));
    }
    Ok(agent_id)
}

/// Forge-relative path to an agent's memory directory.
#[must_use]
pub fn agent_dir(agent_id: &str) -> PathBuf {
    PathBuf::from(AGENTS_DIR).join(agent_id)
}

/// Forge-relative path to an agent's `history.jsonl`.
#[must_use]
pub fn history_path(agent_id: &str) -> PathBuf {
    agent_dir(agent_id).join(HISTORY_FILE)
}

/// Append one entry to the agent's `history.jsonl`.
///
/// Pure filesystem operation — used by callers that already hold a
/// concrete path (CLI / tests). Production callers route through
/// the IPC handler so the kernel applies its capability check first.
///
/// # Errors
/// Returns [`MemoryError::Io`] on write failure or
/// [`MemoryError::Json`] on serialization failure.
pub fn append_entry_to_path(path: &Path, entry: &MemoryEntry) -> Result<(), MemoryError> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| MemoryError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let mut json = serde_json::to_vec(entry).map_err(|source| MemoryError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    json.push(b'\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| MemoryError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(&json).map_err(|source| MemoryError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

/// Read every entry from a `history.jsonl` file, in insertion order.
///
/// Malformed lines are skipped with a `tracing::warn!` rather than
/// aborting the read — the rest of the log stays usable. Missing
/// file is a clean empty result.
///
/// # Errors
/// Returns [`MemoryError::Io`] when the file exists but can't be
/// opened. Per-line JSON parse failures are skipped, not surfaced.
pub fn read_entries_from_path(path: &Path) -> Result<Vec<MemoryEntry>, MemoryError> {
    use std::io::BufRead;
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(MemoryError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let reader = std::io::BufReader::new(file);
    let mut entries = Vec::new();
    for (lineno, line) in reader.lines().enumerate() {
        let line = match line {
            Ok(s) => s,
            Err(source) => {
                return Err(MemoryError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<MemoryEntry>(&line) {
            Ok(e) => entries.push(e),
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    line = lineno + 1,
                    error = %e,
                    "skipping malformed memory entry"
                );
            }
        }
    }
    Ok(entries)
}

/// Filter [`MemoryEntry`] iterators by substring on the entry's
/// principal text field (goal / feedback text, summary, error
/// message, decision summary + rationale, artifact description).
/// Case-insensitive.
pub fn query_entries(
    entries: &[MemoryEntry],
    pattern: &str,
    limit: usize,
) -> Vec<MemoryEntry> {
    let needle = pattern.to_ascii_lowercase();
    let mut out = Vec::new();
    // Newest-first when surfacing recent context to the planner.
    for entry in entries.iter().rev() {
        if out.len() >= limit {
            break;
        }
        let haystack = match entry {
            MemoryEntry::UserGoal { text, .. } | MemoryEntry::UserFeedback { text, .. } => {
                text.to_ascii_lowercase()
            }
            MemoryEntry::StepExecution { summary, .. } => summary.to_ascii_lowercase(),
            MemoryEntry::ToolCall { tool, .. } => tool.to_ascii_lowercase(),
            MemoryEntry::Error { message, .. } => message.to_ascii_lowercase(),
            MemoryEntry::Decision {
                summary, rationale, ..
            } => format!("{summary} {rationale}").to_ascii_lowercase(),
            MemoryEntry::Artifact {
                path, description, ..
            } => format!("{path} {description}").to_ascii_lowercase(),
            MemoryEntry::AgentPlan { plan_id, .. } => plan_id.to_ascii_lowercase(),
        };
        if needle.is_empty() || haystack.contains(&needle) {
            out.push(entry.clone());
        }
    }
    out
}

/// Drop entries older than `retention_ms`. `Decision` entries are
/// preserved unconditionally per PRD-15 §5 ("All decisions retained
/// indefinitely"). Returns the count pruned.
#[must_use]
pub fn prune_entries(entries: Vec<MemoryEntry>, now_ms: u64, retention_ms: u64) -> (Vec<MemoryEntry>, usize) {
    let cutoff = now_ms.saturating_sub(retention_ms);
    let original_len = entries.len();
    let kept: Vec<MemoryEntry> = entries
        .into_iter()
        .filter(|e| e.is_decision() || e.timestamp_ms() >= cutoff)
        .collect();
    let pruned = original_len - kept.len();
    (kept, pruned)
}

/// Render entries as a human-readable markdown export.
#[must_use]
pub fn export_markdown(agent_id: &str, entries: &[MemoryEntry]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    writeln!(out, "# Memory for agent `{agent_id}`").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "{} entries.", entries.len()).unwrap();
    writeln!(out).unwrap();
    for entry in entries {
        let ts = entry.timestamp_ms();
        match entry {
            MemoryEntry::UserGoal { text, .. } => {
                writeln!(out, "- [{ts}] **goal:** {text}").unwrap();
            }
            MemoryEntry::AgentPlan {
                plan_id, step_count, ..
            } => {
                writeln!(out, "- [{ts}] plan `{plan_id}` ({step_count} steps)").unwrap();
            }
            MemoryEntry::StepExecution {
                step_id,
                success,
                summary,
                ..
            } => {
                let ok = if *success { "✓" } else { "✗" };
                writeln!(out, "- [{ts}] {ok} step `{step_id}` — {summary}").unwrap();
            }
            MemoryEntry::ToolCall {
                tool, success, duration_ms, ..
            } => {
                let ok = if *success { "✓" } else { "✗" };
                writeln!(out, "- [{ts}] {ok} tool `{tool}` ({duration_ms}ms)").unwrap();
            }
            MemoryEntry::UserFeedback { text, .. } => {
                writeln!(out, "- [{ts}] **feedback:** {text}").unwrap();
            }
            MemoryEntry::Error { message, step_id, .. } => {
                let s = step_id.as_deref().unwrap_or("-");
                writeln!(out, "- [{ts}] ✗ error in step `{s}`: {message}").unwrap();
            }
            MemoryEntry::Decision {
                summary, rationale, ..
            } => {
                writeln!(out, "- [{ts}] **decision:** {summary}").unwrap();
                writeln!(out, "  - rationale: {rationale}").unwrap();
            }
            MemoryEntry::Artifact {
                path, description, ..
            } => {
                writeln!(out, "- [{ts}] artifact `{path}` — {description}").unwrap();
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_tmp(label: &str) -> PathBuf {
        let tmp = std::env::temp_dir().join(format!(
            "nexus-agent-memory-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        tmp
    }

    fn goal(text: &str, ts: u64) -> MemoryEntry {
        MemoryEntry::UserGoal {
            text: text.to_string(),
            timestamp_ms: ts,
        }
    }

    fn decision(summary: &str, ts: u64) -> MemoryEntry {
        MemoryEntry::Decision {
            summary: summary.to_string(),
            rationale: "because".to_string(),
            timestamp_ms: ts,
        }
    }

    #[test]
    fn normalize_agent_id_accepts_reverse_dns() {
        assert_eq!(
            normalize_agent_id("com.nexus.agent.coder").unwrap(),
            "com.nexus.agent.coder"
        );
    }

    #[test]
    fn normalize_agent_id_rejects_empty() {
        assert!(normalize_agent_id("").is_err());
    }

    #[test]
    fn normalize_agent_id_rejects_slashes() {
        assert!(normalize_agent_id("a/b").is_err());
    }

    #[test]
    fn normalize_agent_id_rejects_too_long() {
        let long = "a".repeat(97);
        assert!(normalize_agent_id(&long).is_err());
    }

    #[test]
    fn append_then_read_round_trips_entries() {
        let tmp = fresh_tmp("rt");
        let path = tmp.join("history.jsonl");
        append_entry_to_path(&path, &goal("first", 1)).unwrap();
        append_entry_to_path(&path, &goal("second", 2)).unwrap();
        let back = read_entries_from_path(&path).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].timestamp_ms(), 1);
        assert_eq!(back[1].timestamp_ms(), 2);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn read_entries_handles_missing_file() {
        let tmp = fresh_tmp("missing");
        let entries = read_entries_from_path(&tmp.join("history.jsonl")).unwrap();
        assert!(entries.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn read_entries_skips_malformed_lines() {
        let tmp = fresh_tmp("malformed");
        let path = tmp.join("history.jsonl");
        std::fs::write(
            &path,
            "{\"kind\":\"user_goal\",\"text\":\"good\",\"timestamp_ms\":1}\nnot json\n",
        )
        .unwrap();
        let back = read_entries_from_path(&path).unwrap();
        assert_eq!(back.len(), 1);
        match &back[0] {
            MemoryEntry::UserGoal { text, .. } => assert_eq!(text, "good"),
            other => panic!("expected UserGoal, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn query_filters_by_substring_case_insensitive() {
        let entries = vec![
            goal("Write README docs", 1),
            goal("Refactor auth code", 2),
            MemoryEntry::ToolCall {
                tool: "write_file".into(),
                success: true,
                duration_ms: 5,
                timestamp_ms: 3,
            },
        ];
        let hits = query_entries(&entries, "README", 10);
        // Returns newest-first, so the matching one should be the
        // only result (the other entries don't contain README).
        assert_eq!(hits.len(), 1);
        match &hits[0] {
            MemoryEntry::UserGoal { text, .. } => assert!(text.contains("README")),
            other => panic!("expected UserGoal, got {other:?}"),
        }
    }

    #[test]
    fn query_empty_pattern_returns_all_newest_first() {
        let entries = vec![goal("first", 1), goal("second", 2), goal("third", 3)];
        let hits = query_entries(&entries, "", 10);
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].timestamp_ms(), 3);
        assert_eq!(hits[2].timestamp_ms(), 1);
    }

    #[test]
    fn query_honours_limit() {
        let entries: Vec<_> = (0..10).map(|i| goal(&format!("g{i}"), i as u64)).collect();
        let hits = query_entries(&entries, "", 3);
        assert_eq!(hits.len(), 3);
    }

    #[test]
    fn prune_drops_old_entries_but_keeps_decisions() {
        // cutoff = now - retention = 2_000 - 1_500 = 500
        //   old (ts=100)        → before cutoff → pruned (not a decision)
        //   old decision (100)  → before cutoff but is_decision → kept
        //   recent (1_000)      → after cutoff → kept
        let entries = vec![
            goal("old goal", 100),
            decision("old decision", 100),
            goal("recent goal", 1_000),
        ];
        let now_ms = 2_000;
        let retention_ms = 1_500;
        let (kept, pruned) = prune_entries(entries, now_ms, retention_ms);
        assert_eq!(pruned, 1, "only the old non-decision goal should be pruned");
        assert_eq!(kept.len(), 2);
        // Decision survived even though it was old.
        assert!(kept.iter().any(MemoryEntry::is_decision));
    }

    #[test]
    fn export_markdown_renders_every_variant() {
        let entries = vec![
            goal("ship docs", 1),
            MemoryEntry::AgentPlan {
                plan_id: "plan-1".into(),
                step_count: 3,
                timestamp_ms: 2,
            },
            MemoryEntry::StepExecution {
                step_id: "s1".into(),
                success: true,
                summary: "wrote intro".into(),
                timestamp_ms: 3,
            },
            MemoryEntry::ToolCall {
                tool: "write_file".into(),
                success: true,
                duration_ms: 5,
                timestamp_ms: 4,
            },
            MemoryEntry::UserFeedback {
                text: "looks great".into(),
                timestamp_ms: 5,
            },
            MemoryEntry::Error {
                message: "build failed".into(),
                step_id: Some("s2".into()),
                timestamp_ms: 6,
            },
            decision("use sentence case", 7),
            MemoryEntry::Artifact {
                path: "docs/intro.md".into(),
                description: "Generated intro".into(),
                timestamp_ms: 8,
            },
        ];
        let md = export_markdown("com.nexus.agent.writer", &entries);
        assert!(md.contains("com.nexus.agent.writer"));
        for needle in [
            "ship docs",
            "plan-1",
            "wrote intro",
            "write_file",
            "looks great",
            "build failed",
            "use sentence case",
            "Generated intro",
        ] {
            assert!(md.contains(needle), "export missing `{needle}`\n{md}");
        }
    }

    #[test]
    fn agent_dir_uses_canonical_path() {
        assert_eq!(
            agent_dir("com.nexus.agent.writer"),
            PathBuf::from(".forge/agents/com.nexus.agent.writer")
        );
    }
}
