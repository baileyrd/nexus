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
    /// BL-120 — compaction event. The session loop summarised
    /// `rounds_compressed` consecutive earlier rounds into a single
    /// `summary` block to keep the live transcript inside the
    /// provider's token budget. Persisting the event in the memory
    /// log gives the next-invocation planner enough breadcrumb to
    /// avoid duplicating prior work even when the per-session
    /// transcript itself has been trimmed.
    CompactedTurns {
        /// How many session rounds the summary replaces. Always
        /// `>= 1`; a zero-round compaction is meaningless and
        /// callers must not record one.
        rounds_compressed: u32,
        /// The summary text the compressor produced. Treated as
        /// opaque prose by the memory layer.
        summary: String,
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
            | Self::Artifact { timestamp_ms, .. }
            | Self::CompactedTurns { timestamp_ms, .. } => *timestamp_ms,
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

/// DG-33 auto-recording — derive the stream of [`MemoryEntry`]
/// values the session loop should auto-record for a completed
/// [`crate::AgentSession`].
///
/// Three classes of entry are emitted:
///
/// - **Compaction events.** `session.compactions` carries the
///   per-rollup `CompactionEvent`s the BL-120 compressor produced;
///   each one becomes a [`MemoryEntry::CompactedTurns`].
///   `rounds_compressed` is `last_round - first_round + 1`. The PRD
///   guarantees `>= 1`.
/// - **Tool calls.** One [`MemoryEntry::ToolCall`] per
///   `ToolCallRecord` in every round. `success = approved &&
///   error.is_empty()`. `duration_ms = 0` for now — the session
///   loop doesn't measure dispatch latency (a future enhancement
///   can populate it). The wall-clock `timestamp_ms` is the value
///   `now_ms` the caller passes in; the session record itself
///   carries no per-call timestamp.
/// - **Errors.** One [`MemoryEntry::Error`] per tool call whose
///   `error` field is non-empty (denied or dispatch-failed), plus
///   one session-level error when `outcome` is
///   [`crate::session::SessionOutcome::Errored`].
///
/// `now_ms` is taken once at call time and reused for every emitted
/// entry so they sort together when interleaved with timestamps
/// from `CompactionEvent` (which the loop stamped at compression
/// time). Callers should call this exactly once per finished
/// session.
///
/// Pure — no I/O, no clocks. The caller provides `now_ms`. Returns
/// an empty `Vec` for a session with no rounds + no compactions +
/// no error outcome, so the caller can short-circuit on
/// `is_empty()` without further checks.
#[must_use]
pub fn events_from_session(
    session: &crate::session::AgentSession,
    now_ms: u64,
) -> Vec<MemoryEntry> {
    let mut out = Vec::new();

    // Compactions first — they ran before the final rounds and
    // carry their own timestamp from the compressor.
    for c in &session.compactions {
        let rounds_compressed = c
            .last_round
            .saturating_sub(c.first_round)
            .saturating_add(1);
        if rounds_compressed == 0 {
            // Defensive — invariant says >= 1, but a future bug in
            // the compressor shouldn't poison the memory log.
            continue;
        }
        out.push(MemoryEntry::CompactedTurns {
            rounds_compressed,
            summary: c.summary.clone(),
            timestamp_ms: c.timestamp_ms,
        });
    }

    // Tool calls + per-call errors. `duration_ms` is the
    // dispatcher-measured wall-clock latency from
    // `session::dispatch_one`; `0` for denied calls (no dispatch) +
    // for entries loaded from a pre-DG-33-duration on-disk
    // transcript (the serde default rides over the missing field).
    for round in &session.rounds {
        for tc in &round.tool_calls {
            let success = tc.approved && tc.error.is_empty();
            out.push(MemoryEntry::ToolCall {
                tool: tc.name.clone(),
                success,
                duration_ms: tc.duration_ms,
                timestamp_ms: now_ms,
            });
            if !tc.error.is_empty() {
                out.push(MemoryEntry::Error {
                    message: format!("{}: {}", tc.name, tc.error),
                    step_id: Some(tc.id.clone()),
                    timestamp_ms: now_ms,
                });
            }
        }
    }

    // Session-level error outcome.
    if matches!(session.outcome, crate::session::SessionOutcome::Errored) {
        out.push(MemoryEntry::Error {
            message: format!("session {} ended with outcome=Errored", session.id),
            step_id: None,
            timestamp_ms: now_ms,
        });
    }

    out
}

/// DG-33 follow-up — render an agent's recent memory entries as a
/// short markdown preamble suitable for splicing into a session's
/// system prompt. Returns `None` when nothing recall-worthy exists.
///
/// **Selection rule** (newest-first):
/// - Every [`MemoryEntry::Decision`] entry up to `decision_cap` —
///   PRD-15 §5 marks decisions as load-bearing context that survives
///   `prune`; they're the highest-signal recall items.
/// - The most recent `recent_cap` non-decision entries
///   ([`MemoryEntry::Error`], [`MemoryEntry::CompactedTurns`],
///   [`MemoryEntry::ToolCall`], etc.) for "what's been happening
///   lately" context.
///
/// Duplicate entries are not removed — the underlying log is
/// append-only; if the same decision is recorded twice it shows
/// twice. Caps the two pools independently so a session full of
/// recent ToolCalls doesn't squeeze out the decision history.
///
/// **Output shape** is a single string suitable for appending to a
/// system prompt with a blank-line separator. Empty input or
/// caps-of-zero return `None` so callers can skip the splice
/// without checking the string.
#[must_use]
pub fn format_memory_preamble(
    entries: &[MemoryEntry],
    decision_cap: usize,
    recent_cap: usize,
) -> Option<String> {
    use std::fmt::Write as _;
    if entries.is_empty() || (decision_cap == 0 && recent_cap == 0) {
        return None;
    }
    // Pre-sort newest-first so the take() below picks the most recent
    // entries from each pool.
    let mut sorted: Vec<&MemoryEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| b.timestamp_ms().cmp(&a.timestamp_ms()));

    let mut decisions: Vec<&MemoryEntry> = Vec::new();
    let mut recent: Vec<&MemoryEntry> = Vec::new();
    for e in &sorted {
        if e.is_decision() {
            if decisions.len() < decision_cap {
                decisions.push(*e);
            }
        } else if recent.len() < recent_cap {
            recent.push(*e);
        }
        if decisions.len() >= decision_cap && recent.len() >= recent_cap {
            break;
        }
    }
    if decisions.is_empty() && recent.is_empty() {
        return None;
    }

    let mut out = String::new();
    out.push_str(
        "## Recent context from prior sessions\n\nUse the entries below as \
         signal from earlier work — files on disk remain the source of truth. \
         Decisions are load-bearing; recent entries surface what happened most \
         recently and may be stale.\n",
    );
    if !decisions.is_empty() {
        out.push_str("\n### Decisions\n");
        for e in &decisions {
            if let MemoryEntry::Decision {
                summary, rationale, ..
            } = e
            {
                if rationale.trim().is_empty() {
                    let _ = writeln!(out, "- {summary}");
                } else {
                    let _ = writeln!(out, "- {summary} — {rationale}");
                }
            }
        }
    }
    if !recent.is_empty() {
        out.push_str("\n### Recent activity\n");
        for e in &recent {
            match e {
                MemoryEntry::Error { message, step_id, .. } => {
                    if let Some(sid) = step_id {
                        let _ = writeln!(out, "- ERROR [{sid}]: {message}");
                    } else {
                        let _ = writeln!(out, "- ERROR: {message}");
                    }
                }
                MemoryEntry::CompactedTurns {
                    rounds_compressed,
                    summary,
                    ..
                } => {
                    let _ = writeln!(
                        out,
                        "- COMPACTED {rounds_compressed} rounds: {summary}",
                    );
                }
                MemoryEntry::ToolCall {
                    tool,
                    success,
                    duration_ms,
                    ..
                } => {
                    let marker = if *success { "ok" } else { "FAILED" };
                    if *duration_ms > 0 {
                        let _ =
                            writeln!(out, "- {marker} tool `{tool}` ({duration_ms}ms)");
                    } else {
                        let _ = writeln!(out, "- {marker} tool `{tool}`");
                    }
                }
                MemoryEntry::StepExecution { step_id, success, summary, .. } => {
                    let marker = if *success { "ok" } else { "FAILED" };
                    let _ = writeln!(out, "- {marker} step `{step_id}`: {summary}");
                }
                MemoryEntry::Artifact { path, description, .. } => {
                    let _ = writeln!(out, "- ARTIFACT `{path}`: {description}");
                }
                MemoryEntry::AgentPlan { plan_id, step_count, .. } => {
                    let _ = writeln!(out, "- PLAN `{plan_id}` ({step_count} steps)");
                }
                MemoryEntry::UserGoal { text, .. } => {
                    let _ = writeln!(out, "- GOAL: {text}");
                }
                MemoryEntry::UserFeedback { text, .. } => {
                    let _ = writeln!(out, "- FEEDBACK: {text}");
                }
                MemoryEntry::Decision { .. } => {} // pulled into decisions list
            }
        }
    }
    Some(out)
}

/// Serialise a batch of [`MemoryEntry`] values as newline-terminated
/// JSON. Used by the session-loop auto-record path that appends
/// many entries in one IPC round-trip via the kernel context's
/// `read_file` + `write_file` pair (the same primitive
/// `handle_memory_record` uses for a single entry).
///
/// Empty input returns an empty buffer; callers should check first.
///
/// # Errors
/// Returns [`MemoryError::Json`] if any entry fails to serialise.
/// The path field is the caller-supplied tag — typically the
/// agent's `history.jsonl` — so the error message is locatable.
pub fn serialize_entries_jsonl(
    entries: &[MemoryEntry],
    tag_path: &Path,
) -> Result<Vec<u8>, MemoryError> {
    let mut out = Vec::new();
    for entry in entries {
        let mut line = serde_json::to_vec(entry).map_err(|source| MemoryError::Json {
            path: tag_path.to_path_buf(),
            source,
        })?;
        line.push(b'\n');
        out.extend_from_slice(&line);
    }
    Ok(out)
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
    // BL-121 — capture the soon-to-be entry index by counting the
    // file's existing newline-terminated lines before we append.
    // Falls back to 0 if the file doesn't exist yet, which is the
    // correct index for the first entry.
    let entry_idx_before = count_lines_in_file(path);
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

    // BL-121 — best-effort live index. The transcript store is set
    // by `nexus_agent::transcript_search::initialize`; tests that
    // don't initialise it (and CLI flows that touch JSONL directly
    // without booting the agent plugin) skip the indexing silently.
    if let Some(store) = crate::transcript_search::global() {
        if let Some(agent_id) = derive_agent_id_from_history_path(path) {
            if let Err(err) = store.append_entry(
                &agent_id,
                i64::try_from(entry_idx_before).unwrap_or(i64::MAX),
                entry,
            ) {
                tracing::warn!(
                    %err,
                    path = %path.display(),
                    "BL-121: live transcript index append failed; index will rebuild from disk next boot"
                );
            }
        }
    }
    Ok(())
}

/// Count newline-terminated lines in a file. Returns 0 if the file
/// doesn't exist. BL-121 uses this to compute the entry_idx of the
/// next append for the live FTS index.
fn count_lines_in_file(path: &Path) -> usize {
    use std::io::BufRead;
    let Ok(file) = std::fs::File::open(path) else {
        return 0;
    };
    std::io::BufReader::new(file).lines().count()
}

/// Pull `<agent_id>` out of a `.../.forge/agents/<agent_id>/history.jsonl`
/// path. Returns `None` for paths that don't match the canonical
/// shape so an off-tree caller (e.g. a future BL stashing a custom
/// log under a different prefix) doesn't accidentally pollute the
/// index with a meaningless segment.
fn derive_agent_id_from_history_path(path: &Path) -> Option<String> {
    if path.file_name()?.to_string_lossy() != HISTORY_FILE {
        return None;
    }
    let dir = path.parent()?;
    let agent_id = dir.file_name()?.to_string_lossy().into_owned();
    normalize_agent_id(&agent_id).ok().map(str::to_string)
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
            MemoryEntry::CompactedTurns { summary, .. } => summary.to_ascii_lowercase(),
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
            MemoryEntry::CompactedTurns {
                rounds_compressed,
                summary,
                ..
            } => {
                writeln!(
                    out,
                    "- [{ts}] **compacted {rounds_compressed} rounds:** {summary}"
                )
                .unwrap();
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

    // ── DG-33 follow-up — events_from_session ──────────────────────────────────

    use crate::compression::CompactionEvent;
    use crate::llm::ProposedToolCall;
    use crate::session::{
        AgentSession, RoundRecord, SessionOutcome, ToolCallRecord,
    };
    use crate::ToolCall;

    fn record_with_duration(
        id: &str,
        name: &str,
        approved: bool,
        error: &str,
        duration_ms: u64,
    ) -> ToolCallRecord {
        let mut r = record(id, name, approved, error);
        r.duration_ms = duration_ms;
        r
    }

    fn record(id: &str, name: &str, approved: bool, error: &str) -> ToolCallRecord {
        let _ = ProposedToolCall {
            id: id.to_string(),
            name: name.to_string(),
            tool_call: ToolCall {
                target_plugin_id: "com.nexus.storage".to_string(),
                command_id: name.to_string(),
                args: serde_json::json!({}),
            },
        };
        ToolCallRecord {
            id: id.to_string(),
            name: name.to_string(),
            tool_call: ToolCall {
                target_plugin_id: "com.nexus.storage".to_string(),
                command_id: name.to_string(),
                args: serde_json::json!({}),
            },
            approved,
            reason: String::new(),
            response: None,
            error: error.to_string(),
            duration_ms: 0,
        }
    }

    fn session_with(
        outcome: SessionOutcome,
        rounds: Vec<Vec<ToolCallRecord>>,
        compactions: Vec<CompactionEvent>,
    ) -> AgentSession {
        AgentSession {
            id: "sess-1".to_string(),
            goal: "do thing".to_string(),
            archetype: Some("com.nexus.agent.coder".to_string()),
            started_at: String::new(),
            ended_at: String::new(),
            rounds: rounds
                .into_iter()
                .enumerate()
                .map(|(i, calls)| RoundRecord {
                    round: (i + 1) as u32,
                    text: String::new(),
                    tool_calls: calls,
                })
                .collect(),
            outcome,
            compactions,
        }
    }

    #[test]
    fn events_from_empty_session_is_empty() {
        let s = session_with(SessionOutcome::Complete, vec![], vec![]);
        assert!(events_from_session(&s, 1_700_000_000_000).is_empty());
    }

    #[test]
    fn events_from_session_carries_dispatch_duration() {
        // DG-33 follow-up — the dispatcher-measured `duration_ms` on
        // each `ToolCallRecord` must surface verbatim on the emitted
        // `MemoryEntry::ToolCall` so prompt-time recall can show
        // "tool X took 12ms last time".
        let s = session_with(
            SessionOutcome::Complete,
            vec![vec![
                record_with_duration("a", "read_file", true, "", 7),
                record_with_duration("b", "write_file", true, "ENOSPC", 1234),
            ]],
            vec![],
        );
        let entries = events_from_session(&s, 1_700_000_000_000);
        // 1 ToolCall for `a` + 1 ToolCall + 1 Error for `b`.
        assert_eq!(entries.len(), 3);
        match &entries[0] {
            MemoryEntry::ToolCall {
                tool, duration_ms, ..
            } => {
                assert_eq!(tool, "read_file");
                assert_eq!(*duration_ms, 7);
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
        match &entries[1] {
            MemoryEntry::ToolCall {
                tool, duration_ms, ..
            } => {
                assert_eq!(tool, "write_file");
                assert_eq!(*duration_ms, 1234);
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn preamble_shows_duration_for_measured_tool_calls() {
        let entries = vec![
            MemoryEntry::ToolCall {
                tool: "slow_tool".to_string(),
                success: true,
                duration_ms: 1234,
                timestamp_ms: 10,
            },
            MemoryEntry::ToolCall {
                tool: "unmeasured".to_string(),
                success: true,
                duration_ms: 0,
                timestamp_ms: 5,
            },
        ];
        let out = format_memory_preamble(&entries, 0, 10).unwrap();
        assert!(out.contains("ok tool `slow_tool` (1234ms)"));
        // Zero duration → no parenthetical.
        assert!(out.contains("ok tool `unmeasured`\n"));
        assert!(!out.contains("unmeasured` ("));
    }

    #[test]
    fn events_from_session_emits_one_tool_call_per_record() {
        let s = session_with(
            SessionOutcome::Complete,
            vec![vec![
                record("a", "read_file", true, ""),
                record("b", "write_file", true, ""),
            ]],
            vec![],
        );
        let entries = events_from_session(&s, 1_700_000_000_000);
        assert_eq!(entries.len(), 2);
        for e in &entries {
            match e {
                MemoryEntry::ToolCall { success, .. } => assert!(success),
                other => panic!("expected ToolCall, got {other:?}"),
            }
        }
    }

    #[test]
    fn events_from_session_emits_error_alongside_failed_tool_call() {
        let s = session_with(
            SessionOutcome::Complete,
            vec![vec![record("a", "write_file", true, "ENOSPC")]],
            vec![],
        );
        let entries = events_from_session(&s, 1_700_000_000_000);
        // One ToolCall + one Error.
        assert_eq!(entries.len(), 2);
        match &entries[0] {
            MemoryEntry::ToolCall { success, tool, .. } => {
                assert!(!success);
                assert_eq!(tool, "write_file");
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
        match &entries[1] {
            MemoryEntry::Error { message, step_id, .. } => {
                assert!(message.contains("write_file"));
                assert!(message.contains("ENOSPC"));
                assert_eq!(step_id.as_deref(), Some("a"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn events_from_session_records_denied_call_as_unsuccessful() {
        // approved=false → success=false, error carries the denial
        // reason which also produces an Error entry.
        let s = session_with(
            SessionOutcome::Complete,
            vec![vec![record(
                "x",
                "terminal_send_signal",
                false,
                "denied by policy",
            )]],
            vec![],
        );
        let entries = events_from_session(&s, 1_700_000_000_000);
        assert_eq!(entries.len(), 2);
        match &entries[0] {
            MemoryEntry::ToolCall { success, .. } => assert!(!success),
            other => panic!("expected ToolCall, got {other:?}"),
        }
        assert!(matches!(&entries[1], MemoryEntry::Error { .. }));
    }

    #[test]
    fn events_from_session_emits_compacted_turns_first() {
        let compaction = CompactionEvent {
            first_round: 1,
            last_round: 4,
            summary: "earlier rounds rolled up".to_string(),
            timestamp_ms: 1_700_000_000_000,
        };
        let s = session_with(
            SessionOutcome::Complete,
            vec![vec![record("a", "read_file", true, "")]],
            vec![compaction],
        );
        let entries = events_from_session(&s, 1_700_000_001_000);
        assert_eq!(entries.len(), 2);
        match &entries[0] {
            MemoryEntry::CompactedTurns {
                rounds_compressed,
                summary,
                ..
            } => {
                assert_eq!(*rounds_compressed, 4);
                assert!(summary.contains("rolled up"));
            }
            other => panic!("expected CompactedTurns, got {other:?}"),
        }
        assert!(matches!(&entries[1], MemoryEntry::ToolCall { .. }));
    }

    #[test]
    fn events_from_session_appends_session_level_error_when_outcome_errored() {
        let s = session_with(SessionOutcome::Errored, vec![], vec![]);
        let entries = events_from_session(&s, 1_700_000_000_000);
        assert_eq!(entries.len(), 1);
        match &entries[0] {
            MemoryEntry::Error { message, step_id, .. } => {
                assert!(message.contains("sess-1"));
                assert!(message.contains("Errored"));
                assert!(step_id.is_none());
            }
            other => panic!("expected session-level Error, got {other:?}"),
        }
    }

    // ── DG-33 follow-up — format_memory_preamble ──────────────────────────────

    #[test]
    fn preamble_returns_none_for_empty_entries() {
        assert!(format_memory_preamble(&[], 10, 10).is_none());
    }

    #[test]
    fn preamble_returns_none_when_both_caps_are_zero() {
        let entries = vec![MemoryEntry::Decision {
            summary: "use sentence case".to_string(),
            rationale: "house style".to_string(),
            timestamp_ms: 1,
        }];
        assert!(format_memory_preamble(&entries, 0, 0).is_none());
    }

    #[test]
    fn preamble_lists_decisions_under_their_own_heading() {
        let entries = vec![
            MemoryEntry::Decision {
                summary: "use sentence case".to_string(),
                rationale: "house style".to_string(),
                timestamp_ms: 1_000,
            },
            MemoryEntry::Decision {
                summary: "prefer 4-space indent".to_string(),
                rationale: String::new(),
                timestamp_ms: 2_000,
            },
        ];
        let out = format_memory_preamble(&entries, 10, 10).unwrap();
        assert!(out.contains("## Recent context"));
        assert!(out.contains("### Decisions"));
        assert!(out.contains("use sentence case — house style"));
        // Empty rationale: no trailing em-dash.
        assert!(out.contains("- prefer 4-space indent"));
        assert!(!out.contains("- prefer 4-space indent —"));
    }

    #[test]
    fn preamble_renders_recent_activity_kinds_in_short_form() {
        let entries = vec![
            MemoryEntry::Error {
                message: "ENOSPC".to_string(),
                step_id: Some("s-1".to_string()),
                timestamp_ms: 1,
            },
            MemoryEntry::CompactedTurns {
                rounds_compressed: 5,
                summary: "early rounds".to_string(),
                timestamp_ms: 2,
            },
            MemoryEntry::ToolCall {
                tool: "read_file".to_string(),
                success: true,
                duration_ms: 0,
                timestamp_ms: 3,
            },
            MemoryEntry::ToolCall {
                tool: "write_file".to_string(),
                success: false,
                duration_ms: 0,
                timestamp_ms: 4,
            },
        ];
        let out = format_memory_preamble(&entries, 10, 10).unwrap();
        assert!(out.contains("### Recent activity"));
        assert!(out.contains("ERROR [s-1]: ENOSPC"));
        assert!(out.contains("COMPACTED 5 rounds: early rounds"));
        assert!(out.contains("ok tool `read_file`"));
        assert!(out.contains("FAILED tool `write_file`"));
    }

    #[test]
    fn preamble_caps_decisions_and_recent_independently() {
        let mut entries = Vec::new();
        for i in 0..30 {
            entries.push(MemoryEntry::Decision {
                summary: format!("d-{i}"),
                rationale: String::new(),
                timestamp_ms: 1_000 + i,
            });
        }
        for i in 0..30 {
            entries.push(MemoryEntry::ToolCall {
                tool: format!("t-{i}"),
                success: true,
                duration_ms: 0,
                timestamp_ms: 2_000 + i,
            });
        }
        let out = format_memory_preamble(&entries, 3, 2).unwrap();
        // Only the most-recent 3 decisions (d-29 / d-28 / d-27).
        let decisions_line_count = out
            .lines()
            .filter(|l| l.starts_with("- d-"))
            .count();
        assert_eq!(decisions_line_count, 3);
        // Only the most-recent 2 tool calls.
        let tool_lines = out.lines().filter(|l| l.contains("tool `t-")).count();
        assert_eq!(tool_lines, 2);
        assert!(out.contains("d-29"));
        assert!(out.contains("t-29"));
    }

    #[test]
    fn preamble_sorts_newest_first_across_kinds() {
        let entries = vec![
            MemoryEntry::ToolCall {
                tool: "old".to_string(),
                success: true,
                duration_ms: 0,
                timestamp_ms: 1,
            },
            MemoryEntry::ToolCall {
                tool: "new".to_string(),
                success: true,
                duration_ms: 0,
                timestamp_ms: 100,
            },
        ];
        let out = format_memory_preamble(&entries, 0, 1).unwrap();
        assert!(out.contains("ok tool `new`"));
        assert!(!out.contains("ok tool `old`"));
    }

    #[test]
    fn serialize_entries_jsonl_round_trips_through_parse_memory_lines_equivalent() {
        let entries = vec![
            MemoryEntry::ToolCall {
                tool: "read_file".to_string(),
                success: true,
                duration_ms: 0,
                timestamp_ms: 1,
            },
            MemoryEntry::Error {
                message: "boom".to_string(),
                step_id: Some("step-1".to_string()),
                timestamp_ms: 2,
            },
        ];
        let bytes = serialize_entries_jsonl(&entries, Path::new("/tmp/h.jsonl")).unwrap();
        // Two newline-terminated records.
        assert_eq!(bytes.iter().filter(|b| **b == b'\n').count(), 2);
        let parsed: Vec<MemoryEntry> = bytes
            .split(|b| *b == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        assert_eq!(parsed.len(), 2);
        assert!(matches!(parsed[0], MemoryEntry::ToolCall { .. }));
        assert!(matches!(parsed[1], MemoryEntry::Error { .. }));
    }
}
