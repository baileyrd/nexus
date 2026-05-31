//! Log + diff domain handlers: `log`, `file_log`, `diff_file`,
//! `diff_staged`.
//!
//! #190 / R7 — all four handlers previously emitted ad-hoc `json!`
//! shapes. They now materialise into the typed `GitLogEntry`,
//! `GitDiffHunk`, `GitDiffLine`, and `GitFileDiff` shapes from
//! `crate::ipc`, all `deny_unknown_fields`. `log` also migrates
//! away from the silent-defaulting `limit_arg` helper to typed
//! `GitLogArgs` parsing — typos like `{ limitt: 50 }` now surface
//! as errors instead of silently meaning "default 20".
//!
//! `file_log` / `diff_file` still use `path_arg` for argument
//! parsing (which carries forge-root path validation that the bare
//! `GitPathArgs` doesn't), but their reply payloads are now typed.

use std::path::Path;

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::ipc::{GitDiffHunk, GitDiffLine, GitFileDiff, GitLogArgs, GitLogEntry};
use crate::GitWorkerHandle;

use super::shared::{map_err, parse_args, path_arg, to_value};

const DEFAULT_LOG_LIMIT: u64 = 20;

fn map_log_entry(le: crate::LogEntry) -> GitLogEntry {
    GitLogEntry {
        hash: le.hash,
        author: le.author,
        date: le.date.to_rfc3339(),
        message: le.message,
        parents: le.parents,
    }
}

fn map_hunk(hunk: crate::HunkDiff) -> GitDiffHunk {
    GitDiffHunk {
        old_start: hunk.old_start,
        old_count: hunk.old_count,
        new_start: hunk.new_start,
        new_count: hunk.new_count,
        lines: hunk
            .lines
            .into_iter()
            .map(|l| GitDiffLine {
                kind: format!("{:?}", l.kind),
                content: l.content,
            })
            .collect(),
    }
}

pub(crate) fn log(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let GitLogArgs { limit } = parse_args(args, "log")?;
    let limit = usize::try_from(limit.unwrap_or(DEFAULT_LOG_LIMIT)).unwrap_or(usize::MAX);
    let entries = h.with(move |e| e.log(limit)).map_err(map_err)?;
    let arr: Vec<GitLogEntry> = entries.into_iter().map(map_log_entry).collect();
    to_value(&arr, "log")
}

pub(crate) fn file_log(
    h: &GitWorkerHandle,
    args: &Value,
    forge_root: &Path,
) -> Result<Value, PluginError> {
    // Path validation via the existing `path_arg`. `limit` is parsed
    // off the same `args` value via `as_u64` — keeping the
    // non-strict shape here because `file_log` accepts both `path`
    // and `limit` and we can't strict-parse both through a single
    // `parse_args` without a combined `GitFileLogArgs` shape (a
    // follow-up can add one).
    let path = path_arg(args, forge_root)?;
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or_else(|| {
            usize::try_from(DEFAULT_LOG_LIMIT).expect("default limit fits in usize")
        });
    let entries = h.with(move |e| e.log_file(&path, limit)).map_err(map_err)?;
    let arr: Vec<GitLogEntry> = entries.into_iter().map(map_log_entry).collect();
    to_value(&arr, "file_log")
}

pub(crate) fn diff_file(
    h: &GitWorkerHandle,
    args: &Value,
    forge_root: &Path,
) -> Result<Value, PluginError> {
    let path = path_arg(args, forge_root)?;
    let hunks = h.with(move |e| e.diff_file(&path)).map_err(map_err)?;
    let arr: Vec<GitDiffHunk> = hunks.into_iter().map(map_hunk).collect();
    to_value(&arr, "diff_file")
}

pub(crate) fn diff_staged(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    let diffs = h.with(|e| e.diff_staged()).map_err(map_err)?;
    let arr: Vec<GitFileDiff> = diffs
        .into_iter()
        .map(|(path, hunks)| GitFileDiff {
            path,
            hunks: hunks.into_iter().map(map_hunk).collect(),
        })
        .collect();
    to_value(&arr, "diff_staged")
}
