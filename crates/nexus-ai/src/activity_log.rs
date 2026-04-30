//! BL-037 — AI activity timeline.
//!
//! Per-forge JSONL log of AI interactions persisted at
//! `.forge/ai-activity.log`. Each AI surface (chat, ask, cmd-i, ghost,
//! enrich) records one entry on completion containing prompt, model,
//! surface, files touched, tool calls, outcome, and duration. The
//! shell reads it back through the `com.nexus.ai::activity_list` IPC
//! handler and renders a scrollable pane.
//!
//! Storage choice: plain JSONL on disk so the file is hand-readable
//! and roundtrips cleanly even if the shell isn't running. We rely on
//! `KernelPluginContext::write_file` for path confinement (the file
//! lives inside the forge); the recorder owns a `Mutex` to serialize
//! the read-modify-write window so concurrent AI calls in the same
//! process don't lose entries.
//!
//! v1 has no FTS / `SQLite` indexing — the full log is read into memory
//! on every `activity_list`. The 256 KiB head-truncation cap keeps
//! that cheap (≈1k entries of 250 B each). BL-037 follow-ups can
//! promote storage to a `Tantivy` / `SQLite` index.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use nexus_kernel::{KernelPluginContext, PluginContext};
use nexus_plugins::PluginError;

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

/// Path inside the forge for the activity log.
pub const ACTIVITY_LOG_PATH: &str = ".forge/ai-activity.log";

/// Hard cap on log file size in bytes. When the file would grow past
/// this on append, the oldest entries are dropped (head-truncation)
/// so the file stays approximately at this size. 256 KiB ≈ 1k entries
/// of 250 bytes each — plenty for "what did I do today".
pub const ACTIVITY_LOG_MAX_BYTES: usize = 256 * 1024;

/// Hard cap on prompt text stored. Truncated with ellipsis. Keeps the
/// log file bounded even when the user pastes a very long prompt.
pub const ACTIVITY_PROMPT_MAX_CHARS: usize = 256;

/// Bus topic published after every successful append. Payload is the
/// freshly-recorded [`ActivityEntry`] serialized to JSON. The shell's
/// `nexus.activityTimeline` plugin subscribes to keep its store in
/// sync without polling.
pub const ACTIVITY_APPENDED_TOPIC: &str = "com.nexus.ai.activity_appended";

/// Surface that originated the AI call. Mirrors the in-product UX
/// surface so users (and future analytics) can slice the timeline.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(rename_all = "lowercase")]
pub enum ActivitySurface {
    /// Chat panel (`stream_chat`, mode=chat).
    Chat,
    /// RAG retrieve + chat (`stream_ask`).
    Ask,
    /// Cmd+I command-anywhere overlay (BL-032).
    CmdI,
    /// Inline ghost completion (BL-034, mode=complete).
    Ghost,
    /// Headless single-shot completion (`complete` CLI / mode=complete).
    Complete,
    /// Auto-enrichment on save (BL-045).
    Enrich,
    /// Catch-all when the surface tag is missing or unknown.
    Other,
}

impl ActivitySurface {
    /// Parse a wire string into a surface tag, falling back to
    /// [`ActivitySurface::Other`] for unknown values. Tolerant on
    /// purpose so a future surface added by a community plugin
    /// doesn't crash deserialisation.
    #[must_use]
    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "chat" => Self::Chat,
            "ask" => Self::Ask,
            "cmdi" | "cmd-i" | "cmd_i" => Self::CmdI,
            "ghost" => Self::Ghost,
            "complete" => Self::Complete,
            "enrich" => Self::Enrich,
            _ => Self::Other,
        }
    }
}

/// Outcome of the AI call. Captures success vs failure separately
/// from the (possibly error) free-form `error` string so the UI can
/// flash an error glyph without parsing.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(rename_all = "lowercase")]
pub enum ActivityOutcome {
    /// Provider returned text and the surface accepted it.
    Ok,
    /// Provider errored, network failed, or the surface rejected
    /// the response.
    Error,
    /// User cancelled mid-stream.
    Cancelled,
}

/// One tool call attempted during a chat round. Captures the tool
/// name + ok/error split; the actual input/output is intentionally
/// not persisted (could contain large file contents).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
pub struct ActivityToolCall {
    /// Registered name of the tool (e.g. `read_file`, `write_file`).
    pub name: String,
    /// `false` if the executor reported an error.
    pub ok: bool,
}

/// One entry in the activity timeline. Persisted as a single JSON
/// object per line in `.forge/ai-activity.log`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
pub struct ActivityEntry {
    /// UUID v4 — stable across reads, useful for de-duping.
    pub id: String,
    /// RFC3339 wall-clock timestamp (UTC).
    pub timestamp: String,
    /// Originating session id (matches `com.nexus.ai.stream_*` events).
    pub session_id: String,
    /// Surface that triggered the call.
    pub surface: ActivitySurface,
    /// Provider name (`anthropic` / `openai` / `ollama`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Concrete model id, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Truncated prompt text (last user message). Kept short so the
    /// log file stays bounded.
    pub prompt: String,
    /// Files referenced — RAG sources, tool-call file paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// Tool calls attempted, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ActivityToolCall>,
    /// Final outcome.
    pub outcome: ActivityOutcome,
    /// Error message when `outcome=error`. Free-form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Wall-clock duration in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

impl ActivityEntry {
    /// Construct a minimally-populated entry. Fields the recorder
    /// will fill (`id`, `timestamp`) get sensible defaults; everything
    /// else is up to the caller.
    #[must_use]
    pub fn now(session_id: String, surface: ActivitySurface) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            session_id,
            surface,
            provider: None,
            model: None,
            prompt: String::new(),
            files: Vec::new(),
            tool_calls: Vec::new(),
            outcome: ActivityOutcome::Ok,
            error: None,
            duration_ms: None,
        }
    }
}

/// In-process recorder. Holds an `Arc<KernelPluginContext>` so handler
/// futures clone the handle cheaply. The internal `Mutex<()>` serializes
/// the read-modify-write window so two AI calls finishing at the same
/// instant don't lose entries.
#[derive(Clone)]
pub struct ActivityRecorder {
    ctx: Arc<KernelPluginContext>,
    write_lock: Arc<Mutex<()>>,
}

impl std::fmt::Debug for ActivityRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `KernelPluginContext` doesn't implement `Debug`; emit just
        // the plugin id so log lines stay informative.
        f.debug_struct("ActivityRecorder")
            .field("plugin_id", &self.ctx.plugin_id())
            .finish_non_exhaustive()
    }
}

impl ActivityRecorder {
    /// Create a recorder bound to `ctx`. Cheap.
    #[must_use]
    pub fn new(ctx: Arc<KernelPluginContext>) -> Self {
        Self {
            ctx,
            write_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Append `entry` to the activity log and publish
    /// [`ACTIVITY_APPENDED_TOPIC`]. Best-effort: a failed disk write
    /// emits a `tracing::warn` and returns `Ok(())` so the user's
    /// chat call never fails because the timeline is full / read-only.
    pub async fn append(&self, mut entry: ActivityEntry) {
        truncate_prompt(&mut entry.prompt, ACTIVITY_PROMPT_MAX_CHARS);
        // Grab the lock for the entire RMW window. Holding across an
        // async ctx.write_file is fine — the recorder is only reached
        // from one Tokio runtime so contention reduces to brief disk
        // serialization.
        let _guard = self.write_lock.lock().await;
        let path = PathBuf::from(ACTIVITY_LOG_PATH);
        // Read existing — treat any error as "no log yet" so a fresh
        // forge starts clean. Most common error: file-not-found.
        let existing = self.ctx.read_file(&path).await.unwrap_or_default();
        let line = match serde_json::to_string(&entry) {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(error = %e, "activity log: encode failed; skipping");
                return;
            }
        };
        let mut next = existing;
        if !next.is_empty() && !next.ends_with(b"\n") {
            next.push(b'\n');
        }
        next.extend_from_slice(line.as_bytes());
        next.push(b'\n');
        let trimmed = head_trim_bytes(next, ACTIVITY_LOG_MAX_BYTES);
        // Workaround for `KernelPluginContext::write_file` returning a
        // path with a trailing separator when overwriting an existing
        // file (the underlying validator's `tail = ""` case appends a
        // `/` to the canonical ancestor, which the OS then rejects
        // with EISDIR on `tokio::fs::write`). Deleting first sidesteps
        // the bug — the recorder is the only writer of this file so a
        // stale read between delete + write is impossible.
        let _ = self.ctx.delete_file(&path).await;
        if let Err(e) = self.ctx.write_file(&path, &trimmed).await {
            tracing::warn!(error = %e, "activity log: write failed");
            return;
        }
        // Publish AFTER the disk write succeeds so subscribers can
        // trust that activity_list will return what they just heard.
        if let Ok(payload) = serde_json::to_value(&entry) {
            let _ = self.ctx.publish(ACTIVITY_APPENDED_TOPIC, payload);
        }
    }

    /// Read the entire log, parsed. Newest entries last; the order on
    /// disk is preserved. Returns an empty Vec when the log file
    /// doesn't exist or contains no parseable lines.
    ///
    /// # Errors
    /// Currently never errors — file-not-found, non-UTF8, and corrupt
    /// JSON lines all degrade to "skip and continue". The `Result`
    /// wrapper is preserved for symmetry with future caller paths
    /// (e.g. an FTS-indexed read) that may want to surface real I/O
    /// failures.
    pub async fn read_all(&self) -> Result<Vec<ActivityEntry>, PluginError> {
        let path = PathBuf::from(ACTIVITY_LOG_PATH);
        let Ok(bytes) = self.ctx.read_file(&path).await else {
            return Ok(Vec::new());
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            // Non-UTF8 means the file is corrupt — surface as empty
            // rather than blowing up the read.
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // Skip corrupt lines (e.g. half-written entry from an
            // interrupted process). Don't fail the whole read.
            if let Ok(e) = serde_json::from_str::<ActivityEntry>(line) {
                out.push(e);
            }
        }
        Ok(out)
    }

    /// Delete the log file. Used by the shell's "clear timeline"
    /// affordance. A subsequent `append` re-creates the file. A
    /// missing file is treated as success — the user wanted "no
    /// timeline", and they have one.
    ///
    /// # Errors
    /// Returns [`PluginError::ExecutionFailed`] when the underlying
    /// `delete_file` fails for any reason other than file-not-found
    /// (e.g. capability denied, permission, disk error).
    pub async fn clear(&self) -> Result<(), PluginError> {
        let _guard = self.write_lock.lock().await;
        let path = PathBuf::from(ACTIVITY_LOG_PATH);
        match self.ctx.delete_file(&path).await {
            Ok(()) => Ok(()),
            // File-not-found is fine; everything else is a real
            // failure the caller should see.
            Err(e) if format!("{e}").to_lowercase().contains("no such file") => Ok(()),
            Err(e) => Err(exec_err(format!("activity clear: {e}"))),
        }
    }
}

/// Truncate `s` to at most `max_chars` chars (Unicode-safe), appending
/// an ellipsis when truncated. No-op when the prompt is already short
/// enough.
fn truncate_prompt(s: &mut String, max_chars: usize) {
    if s.chars().count() <= max_chars {
        return;
    }
    // Reserve room for the ellipsis char. `max_chars` is small so
    // this collect is cheap and Unicode-correct.
    let take = max_chars.saturating_sub(1);
    let mut new = String::with_capacity(take + 1);
    for c in s.chars().take(take) {
        new.push(c);
    }
    new.push('…');
    *s = new;
}

/// Head-trim a JSONL-encoded byte buffer so its length does not exceed
/// `cap`. Removes whole lines from the front so each remaining line
/// is still a parseable JSON object. Always preserves at least the
/// final line on disk.
fn head_trim_bytes(bytes: Vec<u8>, cap: usize) -> Vec<u8> {
    if bytes.len() <= cap {
        return bytes;
    }
    // Find the first newline at or after `bytes.len() - cap`. That
    // becomes the new head. If nothing's found (one giant line), keep
    // the buffer as-is — better to overshoot the cap by one line than
    // lose the most recent entry.
    let drop_at = bytes.len() - cap;
    let cut = bytes
        .iter()
        .enumerate()
        .skip(drop_at)
        .find_map(|(i, &b)| if b == b'\n' { Some(i + 1) } else { None });
    match cut {
        Some(idx) if idx < bytes.len() => bytes[idx..].to_vec(),
        _ => bytes,
    }
}

fn exec_err<S: Into<String>>(reason: S) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: super::core_plugin::PLUGIN_ID.to_string(),
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_prompt_unicode_safe_under_limit_is_noop() {
        let mut s = "hello".to_string();
        truncate_prompt(&mut s, 10);
        assert_eq!(s, "hello");
    }

    #[test]
    fn truncate_prompt_preserves_grapheme_boundaries_with_ellipsis() {
        // 5 emoji chars; multi-byte each. Limit 3 → keep 2 + ellipsis.
        let mut s = "🦀🦀🦀🦀🦀".to_string();
        truncate_prompt(&mut s, 3);
        assert_eq!(s.chars().count(), 3);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn truncate_prompt_at_exact_limit_is_noop() {
        let mut s = "abcdef".to_string();
        truncate_prompt(&mut s, 6);
        assert_eq!(s, "abcdef");
    }

    #[test]
    fn head_trim_returns_input_when_under_cap() {
        let buf = b"line1\nline2\n".to_vec();
        let out = head_trim_bytes(buf.clone(), 1024);
        assert_eq!(out, buf);
    }

    #[test]
    fn head_trim_drops_whole_leading_lines() {
        // Each line is 6 bytes (incl. '\n'); cap to keep ~12 bytes.
        let buf = b"line1\nline2\nline3\nline4\n".to_vec();
        let out = head_trim_bytes(buf, 13);
        // Should retain at least the final line; the result must be a
        // suffix of the input split on a newline so every line is
        // still a complete JSON-shaped record.
        let s = std::str::from_utf8(&out).unwrap();
        assert!(s.ends_with("line4\n"));
        assert!(!s.starts_with("line1"));
    }

    #[test]
    fn head_trim_with_no_newline_after_cut_returns_input() {
        // One huge line longer than cap — no newline to cut at, so we
        // overshoot the cap intentionally.
        let buf = vec![b'x'; 200];
        let out = head_trim_bytes(buf.clone(), 50);
        assert_eq!(out, buf);
    }

    #[test]
    fn surface_from_str_lossy_normalizes_known_aliases() {
        assert_eq!(ActivitySurface::from_str_lossy("chat"), ActivitySurface::Chat);
        assert_eq!(ActivitySurface::from_str_lossy("cmdi"), ActivitySurface::CmdI);
        assert_eq!(ActivitySurface::from_str_lossy("cmd-i"), ActivitySurface::CmdI);
        assert_eq!(ActivitySurface::from_str_lossy("cmd_i"), ActivitySurface::CmdI);
        assert_eq!(
            ActivitySurface::from_str_lossy("not-a-surface"),
            ActivitySurface::Other,
        );
    }

    #[test]
    fn entry_round_trips_through_jsonl() {
        let entry = ActivityEntry {
            id: "id-1".into(),
            timestamp: "2026-04-29T00:00:00Z".into(),
            session_id: "sess-1".into(),
            surface: ActivitySurface::Chat,
            provider: Some("anthropic".into()),
            model: Some("claude-sonnet-4-5".into()),
            prompt: "hi".into(),
            files: vec!["notes/a.md".into()],
            tool_calls: vec![ActivityToolCall {
                name: "read_file".into(),
                ok: true,
            }],
            outcome: ActivityOutcome::Ok,
            error: None,
            duration_ms: Some(123),
        };
        let line = serde_json::to_string(&entry).unwrap();
        // Sanity: snake_case + lowercased enums on the wire.
        assert!(line.contains("\"surface\":\"chat\""));
        assert!(line.contains("\"outcome\":\"ok\""));
        // Empty optional fields should not bloat the line.
        assert!(!line.contains("\"error\""));

        let back: ActivityEntry = serde_json::from_str(&line).unwrap();
        assert_eq!(back.id, "id-1");
        assert_eq!(back.surface, ActivitySurface::Chat);
        assert_eq!(back.tool_calls.len(), 1);
    }
}
