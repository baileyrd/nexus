//! Worktree-domain handlers (Phase 5.3 / RFC 0006): `worktree_list`,
//! `worktree_create`, `worktree_remove`.
//!
//! These are the option-agnostic primitives RFC 0006 ships first: every
//! subagent-isolation model needs to create an isolated working copy, list
//! them, and clean them up. The merge step (patch / branch) is deferred to the
//! Option A build, where the merge policy is decided.

use std::path::Path;

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::ipc::{
    GitOk, GitWorktreeCommitArgs, GitWorktreeCommitReply, GitWorktreeCreateArgs,
    GitWorktreeListReply, GitWorktreeRemoveArgs,
};
use crate::GitWorkerHandle;

use super::shared::{map_err, parse_args, to_value};

pub(crate) fn worktree_list(h: &GitWorkerHandle) -> Result<Value, PluginError> {
    let worktrees = h.with(|e| e.list_worktrees()).map_err(map_err)?;
    to_value(&GitWorktreeListReply { worktrees }, "worktree_list")
}

pub(crate) fn worktree_create(
    h: &GitWorkerHandle,
    args: &Value,
    root: &Path,
) -> Result<Value, PluginError> {
    let GitWorktreeCreateArgs { name, branch } = parse_args(args, "worktree_create")?;
    if !is_valid_worktree_name(&name) {
        return Err(fail(
            "worktree_create: name must be non-empty and only letters, digits, '-' or '_'",
        ));
    }
    // Worktrees live in a managed, gitignored location under the forge so a
    // bad `name` can't escape it and the main repo never tracks the files.
    let base = root.join(".forge").join("worktrees");
    std::fs::create_dir_all(&base).map_err(|e| fail(format!("worktree_create: mkdir: {e}")))?;
    let path = base.join(&name);

    let info = h
        .with(move |e| e.create_worktree(&name, &path, branch.as_deref()))
        .map_err(map_err)?;
    to_value(&info, "worktree_create")
}

pub(crate) fn worktree_remove(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let GitWorktreeRemoveArgs { name, force } = parse_args(args, "worktree_remove")?;
    h.with(move |e| e.remove_worktree(&name, force)).map_err(map_err)?;
    to_value(&GitOk { ok: true }, "worktree_remove")
}

pub(crate) fn worktree_commit(h: &GitWorkerHandle, args: &Value) -> Result<Value, PluginError> {
    let GitWorktreeCommitArgs { name, message } = parse_args(args, "worktree_commit")?;
    let commit_hash = h
        .with(move |e| e.commit_worktree(&name, &message))
        .map_err(map_err)?;
    to_value(&GitWorktreeCommitReply { commit_hash }, "worktree_commit")
}

fn is_valid_worktree_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn fail(reason: impl Into<String>) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: crate::core_plugin::PLUGIN_ID.to_string(),
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::is_valid_worktree_name;

    #[test]
    fn name_validation() {
        assert!(is_valid_worktree_name("task-1_a"));
        assert!(!is_valid_worktree_name(""));
        assert!(!is_valid_worktree_name("../escape"));
        assert!(!is_valid_worktree_name("has/slash"));
        assert!(!is_valid_worktree_name("space bar"));
    }
}
