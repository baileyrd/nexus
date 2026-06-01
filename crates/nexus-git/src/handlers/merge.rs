//! Merge/rebase/cherry-pick domain handlers: `merge`, `abort_merge`,
//! `rebase`, `abort_rebase`, `cherry_pick`, `abort_cherry_pick`,
//! `conflict_files`, `conflict_versions`.
//!
//! #190 / R7 — all eight handlers previously read fields off
//! `serde_json::Value` via `key_string` and emitted ad-hoc `json!`
//! shapes. They now use typed `GitMergeArgs` / `GitMergeReply` /
//! `GitRebaseArgs` / `GitRebaseReply` / `GitCherryPickArgs` /
//! `GitCherryPickReply` / `GitConflictFilesReply` /
//! `GitConflictVersionsReply` / `GitOk` from `crate::ipc`, all
//! `deny_unknown_fields`, so typos like `{ branchh: "main" }` now
//! error instead of silently meaning "missing 'branch' argument".

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::ipc::{
    GitCherryPickArgs, GitCherryPickReply, GitConflictFilesReply, GitConflictVersionsReply,
    GitMergeArgs, GitMergeReply, GitOk, GitPathArgs, GitRebaseArgs, GitRebaseReply,
};
use crate::GitWorkerHandle;

use super::shared::{map_err, parse_args, to_value};

pub(crate) fn merge(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let GitMergeArgs { branch } = parse_args(args, "merge")?;
    let r = h.with(move |e| e.merge(&branch)).map_err(map_err)?;
    to_value(
        &GitMergeReply {
            fast_forward: r.fast_forward,
            conflicts: r.conflicts,
            commit_hash: r.commit_hash,
        },
        "merge",
    )
}

pub(crate) fn abort_merge(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    h.with(|e| e.abort_merge()).map_err(map_err)?;
    to_value(&GitOk { ok: true }, "abort_merge")
}

pub(crate) fn rebase(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let GitRebaseArgs { onto } = parse_args(args, "rebase")?;
    let r = h.with(move |e| e.rebase(&onto)).map_err(map_err)?;
    to_value(
        &GitRebaseReply {
            commits_rebased: r.commits_rebased,
            conflicts: r.conflicts,
        },
        "rebase",
    )
}

pub(crate) fn abort_rebase(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    h.with(|e| e.abort_rebase()).map_err(map_err)?;
    to_value(&GitOk { ok: true }, "abort_rebase")
}

pub(crate) fn cherry_pick(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let GitCherryPickArgs { commit } = parse_args(args, "cherry_pick")?;
    let r = h.with(move |e| e.cherry_pick(&commit)).map_err(map_err)?;
    to_value(
        &GitCherryPickReply {
            commit_hash: r.commit_hash,
            conflicts: r.conflicts,
        },
        "cherry_pick",
    )
}

pub(crate) fn abort_cherry_pick(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    h.with(|e| e.abort_cherry_pick()).map_err(map_err)?;
    to_value(&GitOk { ok: true }, "abort_cherry_pick")
}

pub(crate) fn conflict_files(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    let files = h.with(|e| e.conflict_files()).map_err(map_err)?;
    to_value(&GitConflictFilesReply { files }, "conflict_files")
}

pub(crate) fn conflict_versions(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    // `GitPathArgs { path }` — same shape used by the staging handlers
    // for plain `{ path }` parse; this handler doesn't need the
    // forge-root path validation that the staging variants run, since
    // libgit2 looks up the path against the index, not the
    // filesystem.
    let GitPathArgs { path } = parse_args(args, "conflict_versions")?;
    let v = h
        .with(move |e| e.conflict_versions(&path))
        .map_err(map_err)?;
    to_value(
        &GitConflictVersionsReply {
            base: v.base,
            ours: v.ours,
            theirs: v.theirs,
        },
        "conflict_versions",
    )
}
