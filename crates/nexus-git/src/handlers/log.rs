//! Log + diff domain handlers: `log`, `file_log`, `diff_file`,
//! `diff_staged`.

use std::path::Path;

use nexus_plugins::PluginError;
use serde_json::{json, Value};

use crate::GitWorkerHandle;

use super::shared::{map_err, path_arg};

fn limit_arg(args: &Value, default: usize) -> usize {
    args.get("limit")
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(default)
}

fn log_entry_value(le: &crate::LogEntry) -> Value {
    json!({
        "hash": le.hash,
        "author": le.author,
        "date": le.date.to_rfc3339(),
        "message": le.message,
        "parents": le.parents,
    })
}

fn hunk_value(hunk: &crate::HunkDiff) -> Value {
    json!({
        "old_start": hunk.old_start,
        "old_count": hunk.old_count,
        "new_start": hunk.new_start,
        "new_count": hunk.new_count,
        "lines": hunk.lines.iter().map(|l| json!({
            "kind": format!("{:?}", l.kind),
            "content": l.content,
        })).collect::<Vec<_>>(),
    })
}

pub(crate) fn log(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let limit = limit_arg(args, 20);
    let entries = h.with(move |e| e.log(limit)).map_err(map_err)?;
    Ok(Value::Array(entries.iter().map(log_entry_value).collect()))
}

pub(crate) fn file_log(
    h: &GitWorkerHandle,
    args: &Value,
    forge_root: &Path,
) -> Result<Value, PluginError> {
    let path = path_arg(args, forge_root)?;
    let limit = limit_arg(args, 20);
    let entries = h
        .with(move |e| e.log_file(&path, limit))
        .map_err(map_err)?;
    Ok(Value::Array(entries.iter().map(log_entry_value).collect()))
}

pub(crate) fn diff_file(
    h: &GitWorkerHandle,
    args: &Value,
    forge_root: &Path,
) -> Result<Value, PluginError> {
    let path = path_arg(args, forge_root)?;
    let hunks = h.with(move |e| e.diff_file(&path)).map_err(map_err)?;
    Ok(Value::Array(hunks.iter().map(hunk_value).collect()))
}

pub(crate) fn diff_staged(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    let diffs = h.with(|e| e.diff_staged()).map_err(map_err)?;
    let arr: Vec<_> = diffs
        .iter()
        .map(|(path, hunks)| {
            json!({
                "path": path,
                "hunks": hunks.iter().map(hunk_value).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(Value::Array(arr))
}
