//! Merge/rebase/cherry-pick domain handlers: `merge`, `abort_merge`,
//! `rebase`, `abort_rebase`, `cherry_pick`, `abort_cherry_pick`,
//! `conflict_files`, `conflict_versions`.

use nexus_plugins::PluginError;
use serde_json::{json, Value};

use crate::GitWorkerHandle;

use super::shared::{key_string, map_err};

pub(crate) fn merge(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let branch = key_string(args, "branch")?;
    let r = h.with(move |e| e.merge(&branch)).map_err(map_err)?;
    Ok(json!({
        "fast_forward": r.fast_forward,
        "conflicts": r.conflicts,
        "commit_hash": r.commit_hash,
    }))
}

pub(crate) fn abort_merge(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    h.with(|e| e.abort_merge()).map_err(map_err)?;
    Ok(json!({"ok": true}))
}

pub(crate) fn rebase(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let onto = key_string(args, "onto")?;
    let r = h.with(move |e| e.rebase(&onto)).map_err(map_err)?;
    Ok(json!({
        "commits_rebased": r.commits_rebased,
        "conflicts": r.conflicts,
    }))
}

pub(crate) fn abort_rebase(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    h.with(|e| e.abort_rebase()).map_err(map_err)?;
    Ok(json!({"ok": true}))
}

pub(crate) fn cherry_pick(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let commit = key_string(args, "commit")?;
    let r = h.with(move |e| e.cherry_pick(&commit)).map_err(map_err)?;
    Ok(json!({
        "commit_hash": r.commit_hash,
        "conflicts": r.conflicts,
    }))
}

pub(crate) fn abort_cherry_pick(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    h.with(|e| e.abort_cherry_pick()).map_err(map_err)?;
    Ok(json!({"ok": true}))
}

pub(crate) fn conflict_files(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    let files = h.with(|e| e.conflict_files()).map_err(map_err)?;
    Ok(json!({"files": files}))
}

pub(crate) fn conflict_versions(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let path = key_string(args, "path")?;
    let v = h
        .with(move |e| e.conflict_versions(&path))
        .map_err(map_err)?;
    // Bytes go over the wire as JSON arrays of u8 — the shell decodes
    // to a Uint8Array, then to text or binary preview as appropriate.
    Ok(json!({
        "base":   v.base,
        "ours":   v.ours,
        "theirs": v.theirs,
    }))
}
