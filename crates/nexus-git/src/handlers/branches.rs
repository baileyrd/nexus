//! Branch-domain handlers: `branches`, `switch_branch`,
//! `create_branch`, `delete_branch`, `push`.
//!
//! #190 / R7 — these handlers previously read `name` / `remote` /
//! `branch` off `serde_json::Value` via `key_string` and emitted
//! ad-hoc `json!({"ok": true})` replies, so they were invisible to
//! both the `ipc_strictness` gate and the schemars schema generator.
//! Routing through `parse_args::<GitBranchArgs | GitPushArgs>` and
//! `to_value(&GitOk { ok: true })` brings them under the same
//! `deny_unknown_fields` + drift guarantees the rest of the storage
//! handlers already have.

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::ipc::{
    GitBranch, GitBranchArgs, GitFetchArgs, GitMergeReply, GitOk, GitPushArgs, GitRemotesReply,
};
use crate::GitWorkerHandle;

use super::shared::{map_err, parse_args, to_value};

pub(crate) fn branches(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    let branches = h.with(|e| e.branches()).map_err(map_err)?;
    // Materialise into the typed wire shape so the schemars schema
    // generator sees the same fields the runtime emits.
    let arr: Vec<GitBranch> = branches
        .into_iter()
        .map(|b| GitBranch {
            name: b.name,
            is_head: b.is_head,
            upstream: b.upstream,
        })
        .collect();
    to_value(&arr, "branches")
}

pub(crate) fn switch_branch(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let GitBranchArgs { name } = parse_args(args, "switch_branch")?;
    h.with(move |e| e.switch_branch(&name)).map_err(map_err)?;
    to_value(&GitOk { ok: true }, "switch_branch")
}

pub(crate) fn create_branch(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let GitBranchArgs { name } = parse_args(args, "create_branch")?;
    h.with(move |e| e.create_branch(&name)).map_err(map_err)?;
    to_value(&GitOk { ok: true }, "create_branch")
}

pub(crate) fn delete_branch(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let GitBranchArgs { name } = parse_args(args, "delete_branch")?;
    h.with(move |e| e.delete_branch(&name)).map_err(map_err)?;
    to_value(&GitOk { ok: true }, "delete_branch")
}

pub(crate) fn push(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let GitPushArgs { remote, branch } = parse_args(args, "push")?;
    h.with(move |e| e.push(&remote, &branch)).map_err(map_err)?;
    to_value(&GitOk { ok: true }, "push")
}

/// C49 (#425) — list configured remote names. Read-only local
/// enumeration (no network access itself).
pub(crate) fn remotes(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    let remotes = h.with(|e| e.remotes()).map_err(map_err)?;
    to_value(&GitRemotesReply { remotes }, "remotes")
}

/// C49 (#425) — fetch all refs from a remote. Same credential
/// machinery as `push` (SSH-agent → default keys → keyring
/// passphrase → libgit2 default).
pub(crate) fn fetch(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let GitFetchArgs { remote } = parse_args(args, "fetch")?;
    h.with(move |e| e.fetch(&remote)).map_err(map_err)?;
    to_value(&GitOk { ok: true }, "fetch")
}

/// C49 (#425) — fetch + merge a remote tracking branch. Reuses
/// `GitPushArgs` (same `{remote, branch}` shape as `push`) and
/// `GitMergeReply` (same result shape as `merge`, since `pull` is
/// `fetch` followed by a merge against `<remote>/<branch>`).
pub(crate) fn pull(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let GitPushArgs { remote, branch } = parse_args(args, "pull")?;
    let r = h.with(move |e| e.pull(&remote, &branch)).map_err(map_err)?;
    to_value(
        &GitMergeReply {
            fast_forward: r.fast_forward,
            conflicts: r.conflicts,
            commit_hash: r.commit_hash,
        },
        "pull",
    )
}
