//! Stash-domain handlers: `stash_push`, `stash_list`, `stash_pop`,
//! `stash_drop`.
//!
//! #190 / R7 — these handlers previously read `message` / `index`
//! off `serde_json::Value` via a private `index_arg` helper that
//! silently defaulted to `0` on missing/invalid input, and emitted
//! ad-hoc `json!({"ok": true, ...})` replies. The typed shapes
//! (`GitStashPushArgs`, `GitStashIndexArgs`, `GitStashPushReply`,
//! `GitStashEntry`, `GitOk`) live in `crate::ipc`, all
//! `deny_unknown_fields`. This routes the handlers through them so
//! the family is policed by the `ipc_strictness` gate.

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::ipc::{GitOk, GitStashEntry, GitStashIndexArgs, GitStashPushArgs, GitStashPushReply};
use crate::GitWorkerHandle;

use super::shared::{map_err, parse_args, to_value};

pub(crate) fn stash_push(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let GitStashPushArgs { message } = parse_args(args, "stash_push")?;
    let idx = h
        .with(move |e| e.stash_push(message.as_deref()))
        .map_err(map_err)?;
    to_value(
        &GitStashPushReply {
            ok: true,
            index: idx,
        },
        "stash_push",
    )
}

pub(crate) fn stash_list(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    let entries = h.with(|e| e.stash_list()).map_err(map_err)?;
    let arr: Vec<GitStashEntry> = entries
        .into_iter()
        .map(|s| GitStashEntry {
            index: s.index,
            message: s.message,
            oid: s.oid,
        })
        .collect();
    to_value(&arr, "stash_list")
}

pub(crate) fn stash_pop(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let GitStashIndexArgs { index } = parse_args(args, "stash_pop")?;
    let idx = index.unwrap_or(0);
    h.with(move |e| e.stash_pop(idx)).map_err(map_err)?;
    to_value(&GitOk { ok: true }, "stash_pop")
}

pub(crate) fn stash_drop(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let GitStashIndexArgs { index } = parse_args(args, "stash_drop")?;
    let idx = index.unwrap_or(0);
    h.with(move |e| e.stash_drop(idx)).map_err(map_err)?;
    to_value(&GitOk { ok: true }, "stash_drop")
}
