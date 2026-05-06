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
