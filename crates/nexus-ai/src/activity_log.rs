//! BL-037 — AI activity log recorder.
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
//!
//! BL-052 — type definitions (`ActivityEntry`, `ActivitySurface`,
//! `ActivityOutcome`, `ActivityOrigin`, `ActivityToolCall`) live in
//! `nexus_types::activity` so other emitters can publish without
//! depending on this crate. The recorder still owns the on-disk JSONL
//! log for AI surfaces and publishes to both the universal topic and
//! the legacy AI-only topic.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;

use nexus_kernel::{Events as _, FileSystem as _, Identity as _, KernelPluginContext};
use nexus_plugins::PluginError;
use nexus_types::activity::{
    truncate_prompt, ActivityEntry, ACTIVITY_APPENDED_TOPIC, ACTIVITY_PROMPT_MAX_CHARS,
    AI_ACTIVITY_APPENDED_TOPIC,
};

/// Path inside the forge for the AI activity log.
pub const ACTIVITY_LOG_PATH: &str = ".forge/ai-activity.log";

/// Hard cap on log file size in bytes. When the file would grow past
/// this on append, the oldest entries are dropped (head-truncation)
/// so the file stays approximately at this size. 256 KiB ≈ 1k entries
/// of 250 bytes each — plenty for "what did I do today".
pub const ACTIVITY_LOG_MAX_BYTES: usize = 256 * 1024;

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

    /// Append `entry` to the activity log and publish to both the
    /// universal [`ACTIVITY_APPENDED_TOPIC`] and the legacy
    /// [`AI_ACTIVITY_APPENDED_TOPIC`]. Best-effort: a failed disk
    /// write emits a `tracing::warn` and returns silently so the
    /// user's chat call never fails because the timeline is full or
    /// read-only.
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
        // BL-052 — fire BOTH topics:
        //   * universal `com.nexus.activity.appended` for the BL-052
        //     timeline that aggregates across emitters
        //   * legacy `com.nexus.ai.activity_appended` for any
        //     subscriber that still listens on the AI-only topic.
        if let Ok(payload) = serde_json::to_value(&entry) {
            let _ = self.ctx.publish(ACTIVITY_APPENDED_TOPIC, payload.clone());
            let _ = self.ctx.publish(AI_ACTIVITY_APPENDED_TOPIC, payload);
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
    let cut = bytes.iter().enumerate().skip(drop_at).find_map(|(i, &b)| {
        if b == b'\n' {
            Some(i + 1)
        } else {
            None
        }
    });
    match cut {
        Some(idx) if idx < bytes.len() => bytes[idx..].to_vec(),
        _ => bytes,
    }
}

use crate::handlers::shared::exec_err;

#[cfg(test)]
mod tests {
    use super::*;

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
}
