//! Wire-mirror IPC types for `com.nexus.git`.
//!
//! Audit-2026-05-01 P1-3 (#113). The handlers in
//! [`crate::core_plugin`] currently construct responses with ad-hoc
//! `serde_json::json!` macros — there are no named arg/reply types
//! to gate. The impl types in [`crate::types`] don't even derive
//! `Serialize`. Same wire-mirror pattern as `nexus_storage::ipc` and
//! `nexus_mcp::ipc`.

use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

// ── Args ─────────────────────────────────────────────────────────────────────

/// Args for `com.nexus.git::log` (handler id `2`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitLogArgs {
    /// Maximum number of entries to return, newest first. Omit for
    /// the default of 20.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
}

/// Args for `com.nexus.git::file_status`, `diff_file`, `stage_file`,
/// `unstage_file` (handler ids `4`, `5`, `6`, `7`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitPathArgs {
    /// Forge-relative path of the file. Path-traversal attempts
    /// (`..`) and absolute paths are rejected by the engine.
    pub path: String,
}

/// Args for `com.nexus.git::commit` (handler id `8`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitCommitArgs {
    /// Commit message. Forwarded verbatim to libgit2.
    pub message: String,
}

// ── Replies ──────────────────────────────────────────────────────────────────

/// Return type for `com.nexus.git::status` (handler id `1`). Mirrors
/// [`crate::types::GitState`] for the wire (the engine emits
/// `repo_state` as a stringified Debug of [`crate::types::RepoState`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitStatusReply {
    /// Current branch name, or `null` if HEAD is detached.
    pub branch: Option<String>,
    /// Short hex of HEAD commit (or `"(none)"` for empty repos).
    pub head: String,
    /// Whether the working tree has uncommitted changes.
    pub is_dirty: bool,
    /// Stringified repo state (`Clean`, `Merge`, `Rebase`,
    /// `RebaseInteractive`, `CherryPick`, `Revert`, `Bisect`).
    pub repo_state: String,
}

/// One entry in the `log` handler's response array. Mirrors
/// [`crate::types::LogEntry`] for the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitLogEntry {
    /// Short hex commit hash.
    pub hash: String,
    /// Author name.
    pub author: String,
    /// Commit date as RFC3339 string.
    pub date: String,
    /// Full commit message.
    pub message: String,
    /// Parent commit hashes.
    pub parents: Vec<String>,
}

/// One entry in the `branches` handler's response array. Mirrors
/// [`crate::types::BranchInfo`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitBranch {
    /// Branch name.
    pub name: String,
    /// `true` if this is the currently checked-out branch.
    pub is_head: bool,
    /// Upstream tracking branch name (e.g. `"origin/main"`).
    pub upstream: Option<String>,
}

/// One line in a diff hunk. Mirrors [`crate::types::DiffLine`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitDiffLine {
    /// Stringified line kind (`Context`, `Added`, `Removed`).
    pub kind: String,
    /// Line content (without trailing newline).
    pub content: String,
}

/// One hunk in the `diff_file` handler's response array. Mirrors
/// [`crate::types::HunkDiff`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitDiffHunk {
    /// Start line in the old file (1-based).
    pub old_start: u32,
    /// Number of lines in the old version.
    pub old_count: u32,
    /// Start line in the new file (1-based).
    pub new_start: u32,
    /// Number of lines in the new version.
    pub new_count: u32,
    /// Lines in this hunk, in order.
    pub lines: Vec<GitDiffLine>,
}

/// Return type for `stage_file`, `unstage_file`, `stage_all`,
/// `unstage_all` (handler ids `6`, `7`, `9`, `10`). The engine emits
/// `{"ok": true}` for every successful call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitOk {
    /// Always `true` when the wrapped operation succeeded.
    pub ok: bool,
}

/// Return type for `com.nexus.git::commit` (handler id `8`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitCommitReply {
    /// Short hex hash of the newly-created commit.
    pub hash: String,
}

// ── New handlers added for the git panel (BL-084) ────────────────────────────

/// One entry in the `file_statuses` response (handler id `11`).
/// Status is the `Debug` string of [`crate::types::FileStatus`]:
/// `"Untracked"`, `"Modified"`, `"Staged"`, `"Removed"`,
/// `"Renamed"`, `"Conflicted"`, or `"Added"`. Unmodified files
/// are excluded by the engine's status options.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitFileStatus {
    /// Forge-relative path of the file.
    pub path: String,
    /// One of: `"Untracked"`, `"Modified"`, `"Staged"`,
    /// `"Removed"`, `"Renamed"`, `"Conflicted"`, `"Added"`.
    pub status: String,
}

/// One file with its staged diff hunks. Used in the `diff_staged`
/// response array (handler id `12`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitFileDiff {
    /// Forge-relative path.
    pub path: String,
    /// Diff hunks for this file.
    pub hunks: Vec<GitDiffHunk>,
}

/// Args for `switch_branch` (13), `create_branch` (14),
/// `delete_branch` (15).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitBranchArgs {
    /// Branch name to operate on.
    pub name: String,
}

/// Args for `worktree_create` (handler id `40`). Phase 5.3 / RFC 0006 —
/// the worktree lands at `<forge>/.forge/worktrees/<name>` (a managed,
/// gitignored location); the caller chooses only the `name` and an optional
/// branch to check out (created at HEAD when absent).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitWorktreeCreateArgs {
    /// Worktree name (a simple slug: letters, digits, `-`, `_`).
    pub name: String,
    /// Branch to check out in the worktree; created at HEAD if it does not
    /// exist. Defaults to a branch named after the worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

/// Args for `worktree_remove` (handler id `41`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitWorktreeRemoveArgs {
    /// Worktree name to remove.
    pub name: String,
    /// Remove even when the worktree is valid (deletes its working tree on
    /// disk). When false, only stale/prunable worktrees are removed.
    #[serde(default)]
    pub force: bool,
}

/// One worktree in the `worktree_list` reply (handler id `39`) and the
/// `worktree_create` reply.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitWorktreeInfo {
    /// Worktree name.
    pub name: String,
    /// Absolute path of the worktree's working directory.
    pub path: String,
    /// Branch checked out in the worktree, if it could be resolved.
    pub branch: Option<String>,
}

/// Reply for `worktree_list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitWorktreeListReply {
    /// Worktrees attached to the forge repository.
    pub worktrees: Vec<GitWorktreeInfo>,
}

/// Args for `worktree_commit` (handler id `42`, RFC 0007) — stage every change
/// in a worktree's working tree and commit it to that worktree's checked-out
/// branch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitWorktreeCommitArgs {
    /// Worktree name to commit (as passed to `worktree_create`).
    pub name: String,
    /// Commit message.
    pub message: String,
}

/// Reply for `worktree_commit`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitWorktreeCommitReply {
    /// Short hash of the new commit, or `None` when the worktree had no
    /// changes to commit (the staged tree matched its `HEAD`).
    pub commit_hash: Option<String>,
}

/// One entry in the `stash_list` response (handler id `24`). Mirrors
/// [`crate::types::StashEntry`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitStashEntry {
    /// 0-based position in the stash stack (0 = most recent).
    pub index: usize,
    /// Human-readable stash message.
    pub message: String,
    /// Short hex hash of the stash commit.
    pub oid: String,
}

/// Args for `stash_push` (handler id `23`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitStashPushArgs {
    /// Optional stash message. A default `"WIP on <branch>: <head>"` is
    /// generated if omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Args for `stash_pop` and `stash_drop` (handler ids `25`, `26`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitStashIndexArgs {
    /// 0-based stash index. Defaults to `0` (most recent) when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
}

/// Return type for `stash_push` (handler id `23`). #190 — typed
/// counterpart of the prior `json!({"ok": true, "index": idx})`
/// reply. The `index` is the 0-based position of the new stash
/// entry in the stack (always 0 on success since `git stash push`
/// pushes onto the top).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitStashPushReply {
    /// Always `true` when the stash was created.
    pub ok: bool,
    /// 0-based stack position of the new stash entry.
    pub index: usize,
}

/// Args for `stage_hunks` (17) and `unstage_hunks` (18).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitHunkArgs {
    /// Forge-relative path of the file.
    pub path: String,
    /// 0-based indices of the hunks to stage or unstage.
    pub hunk_indices: Vec<u64>,
}

/// One tag in the `list_tags` response (handler id `19`). Mirrors
/// [`crate::types::TagInfo`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitTagInfo {
    /// Short tag name (e.g. `"v1.0.0"`).
    pub name: String,
    /// Short hex hash of the tagged commit.
    pub target_hash: String,
    /// `true` for annotated tags.
    pub is_annotated: bool,
    /// Tag message (annotated tags only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Args for `create_tag` (handler id `20`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitCreateTagArgs {
    /// Short tag name (e.g. `"v1.0.0"`).
    pub name: String,
    /// Tag message. Provide to create an annotated tag; omit for lightweight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Args for `delete_tag` (handler id `21`). #190 — typed counterpart
/// of the prior `key_string(args, "name")` lookup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitDeleteTagArgs {
    /// Short tag name to delete.
    pub name: String,
}

/// Args for `push_tags` (handler id `22`). #190 — typed counterpart
/// of the prior `key_string(args, "remote")` lookup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitPushTagsArgs {
    /// Remote name (e.g. `"origin"`).
    pub remote: String,
}

/// Args for `push` (handler id `16`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitPushArgs {
    /// Remote name (e.g. `"origin"`).
    pub remote: String,
    /// Branch name to push (e.g. `"main"`).
    pub branch: String,
}

/// BL-079 — one entry from `com.nexus.git::blame`. Wire-mirror of
/// [`crate::types::BlameEntry`] with serde / ts-rs / JsonSchema
/// derivations the impl type doesn't carry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitBlameEntry {
    /// Short (7-char) commit hash that last touched this line range.
    pub commit_hash: String,
    /// Author name from the commit's signature.
    pub author: String,
    /// ISO-8601 commit date (UTC).
    pub date: String,
    /// First line of the commit message — the "summary".
    pub message: String,
    /// 1-based start line of the blame hunk in the current file.
    pub start_line: u32,
    /// 1-based end line of the blame hunk in the current file.
    pub end_line: u32,
}

// ── #190: merge / rebase / cherry-pick wire shapes ──────────────────────────

/// Args for `merge` (handler id `25`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitMergeArgs {
    /// Branch name to merge into HEAD.
    pub branch: String,
}

/// Return type for `merge` (handler id `25`). Wire mirror of
/// [`crate::types::MergeResult`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitMergeReply {
    /// `true` if the merge was a fast-forward.
    pub fast_forward: bool,
    /// Files with unresolved conflicts (empty if none).
    pub conflicts: Vec<String>,
    /// Commit hash of the merge commit; `null` on conflict or up-to-date.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_hash: Option<String>,
}

/// Args for `rebase` (handler id `27`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitRebaseArgs {
    /// Commit / branch to rebase onto.
    pub onto: String,
}

/// Return type for `rebase` (handler id `27`). Wire mirror of
/// [`crate::types::RebaseResult`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitRebaseReply {
    /// Number of commits successfully replayed onto the new base.
    pub commits_rebased: u32,
    /// Files with unresolved conflicts.
    pub conflicts: Vec<String>,
}

/// Args for `cherry_pick` (handler id `29`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitCherryPickArgs {
    /// Commit hash to cherry-pick onto HEAD.
    pub commit: String,
}

/// Return type for `cherry_pick` (handler id `29`). Wire mirror of
/// [`crate::types::CherryPickResult`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitCherryPickReply {
    /// Files with unresolved conflicts.
    pub conflicts: Vec<String>,
    /// Hash of the new commit; `null` on conflicts or noop.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_hash: Option<String>,
}

/// Return type for `conflict_files` (handler id `31`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitConflictFilesReply {
    /// Forge-relative paths of every file with unresolved conflicts.
    pub files: Vec<String>,
}

/// Return type for `conflict_versions` (handler id `32`). Bytes go
/// over the wire as JSON arrays of u8; the shell decodes to a
/// `Uint8Array` then to text / binary preview as appropriate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitConflictVersionsReply {
    /// Common ancestor (stage 1). `null` when there is no shared base.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<Vec<u8>>,
    /// HEAD side (stage 2 — "ours"). `null` when the file was deleted
    /// on our side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ours: Option<Vec<u8>>,
    /// Incoming side (stage 3 — "theirs"). `null` when the file was
    /// deleted on the incoming side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theirs: Option<Vec<u8>>,
}

/// Args for `file_log` (handler id `13`). #190 — typed counterpart of
/// the prior `{path, limit?}` hand-rolled parse. `path` is required;
/// `limit` defaults to 20 inside the handler when omitted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitFileLogArgs {
    /// Forge-relative path of the file.
    pub path: String,
    /// Maximum number of entries to return, newest first. Omit for
    /// the default of 20.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
}

/// Return type for `lfs_status` (handler id `36`). #190 — typed
/// counterpart of the prior `json!({"tracked_patterns": …,
/// "pointer_files": …, "available_files": …, "git_lfs_installed": …})`
/// reply.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GitLfsStatusReply {
    /// Patterns listed in `<forge>/.gitattributes` with `filter=lfs`.
    pub tracked_patterns: Vec<String>,
    /// LFS-tracked files that are present only as pointers locally.
    pub pointer_files: Vec<String>,
    /// LFS-tracked files whose actual bytes are materialised locally.
    pub available_files: Vec<String>,
    /// `true` when the `git-lfs` binary is on the host's `PATH`.
    /// `false` collapses `pointer_files` / `available_files` to empty
    /// lists (we know LFS is in use here but cannot inspect
    /// availability).
    pub git_lfs_installed: bool,
}
