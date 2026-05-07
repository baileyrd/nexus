//! Data types for git state, status, diffs, blame, and log.

use std::path::PathBuf;

use chrono::{DateTime, Utc};

/// High-level repository state.
#[derive(Debug, Clone)]
pub struct GitState {
    /// Current branch name, or `None` if HEAD is detached.
    pub branch: Option<String>,
    /// Short hex of HEAD commit (or `"(none)"` for empty repos).
    pub head_oid: String,
    /// Whether the working tree has uncommitted changes.
    pub is_dirty: bool,
    /// Current repo operation state.
    pub repo_state: RepoState,
}

/// Repository operation state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoState {
    /// Normal — no operation in progress.
    Clean,
    /// Merge in progress.
    Merge,
    /// Rebase in progress.
    Rebase,
    /// Interactive rebase in progress.
    RebaseInteractive,
    /// Cherry-pick in progress.
    CherryPick,
    /// Revert in progress.
    Revert,
    /// Bisect in progress.
    Bisect,
}

/// Git status of a single file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    /// File is not tracked by git.
    Untracked,
    /// File is tracked and unchanged.
    Unmodified,
    /// File has been modified in the working tree.
    Modified,
    /// File has been staged in the index.
    Staged,
    /// File has been deleted.
    Removed,
    /// File has been renamed.
    Renamed,
    /// File has merge conflicts.
    Conflicted,
    /// File has been newly added to the index.
    Added,
}

impl FileStatus {
    /// Single-character status marker for display.
    #[must_use] 
    pub fn marker(self) -> &'static str {
        match self {
            Self::Untracked => "?",
            Self::Unmodified => " ",
            Self::Modified => "M",
            Self::Staged => "S",
            Self::Removed => "D",
            Self::Renamed => "R",
            Self::Conflicted => "C",
            Self::Added => "A",
        }
    }
}

/// A single diff line.
#[derive(Debug, Clone)]
pub struct DiffLine {
    /// Line type.
    pub kind: DiffLineKind,
    /// Line content (without trailing newline).
    pub content: String,
}

/// Kind of diff line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    /// Unchanged context line.
    Context,
    /// Added line.
    Added,
    /// Removed line.
    Removed,
}

/// A contiguous diff hunk.
#[derive(Debug, Clone)]
pub struct HunkDiff {
    /// Start line in the old file (1-based).
    pub old_start: u32,
    /// Number of lines in the old version.
    pub old_count: u32,
    /// Start line in the new file (1-based).
    pub new_start: u32,
    /// Number of lines in the new version.
    pub new_count: u32,
    /// Lines in this hunk.
    pub lines: Vec<DiffLine>,
}

/// A blame annotation for a range of lines.
#[derive(Debug, Clone)]
pub struct BlameEntry {
    /// Short commit hash.
    pub commit_hash: String,
    /// Author name.
    pub author: String,
    /// Commit date.
    pub date: DateTime<Utc>,
    /// First line of commit message.
    pub message: String,
    /// Start line (1-based, inclusive).
    pub start_line: usize,
    /// End line (1-based, inclusive).
    pub end_line: usize,
}

/// A commit log entry.
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Short hex commit hash.
    pub hash: String,
    /// Author name.
    pub author: String,
    /// Commit date.
    pub date: DateTime<Utc>,
    /// Full commit message.
    pub message: String,
    /// Parent commit hashes.
    pub parents: Vec<String>,
}

/// Result of a merge operation.
#[derive(Debug, Clone)]
pub struct MergeResult {
    /// Whether the merge was a fast-forward.
    pub fast_forward: bool,
    /// Files with unresolved conflicts (empty if none).
    pub conflicts: Vec<String>,
    /// Commit hash of the merge commit (None if conflicts or up-to-date).
    pub commit_hash: Option<String>,
}

/// Result of a non-interactive rebase (BL-088).
#[derive(Debug, Clone)]
pub struct RebaseResult {
    /// Number of commits successfully replayed onto the new base.
    /// Zero on a noop / up-to-date rebase or when the first
    /// operation produced conflicts.
    pub commits_rebased: u32,
    /// Files with unresolved conflicts. Non-empty means the rebase
    /// is paused and the caller should either resolve + recommit
    /// or call `abort_rebase`.
    pub conflicts: Vec<String>,
}

/// Result of a cherry-pick (BL-088).
#[derive(Debug, Clone)]
pub struct CherryPickResult {
    /// Files with unresolved conflicts. Non-empty means the
    /// caller must resolve them and commit manually (or abort).
    pub conflicts: Vec<String>,
    /// Hash of the new commit on success; `None` when there were
    /// conflicts or the picked commit was already in HEAD.
    pub commit_hash: Option<String>,
}

/// The three index-side versions of a conflicted file (BL-084).
/// Each side is `None` when libgit2 does not record an entry on
/// that stage (e.g. a file added on one side only has no `base`).
/// Bytes are the blob contents at that stage; the caller decodes
/// to text or renders binary as appropriate.
#[derive(Debug, Clone, Default)]
pub struct ConflictVersions {
    /// Common ancestor (stage 1). `None` when there is no shared base.
    pub base: Option<Vec<u8>>,
    /// HEAD side (stage 2 — "ours"). `None` when the file was
    /// deleted on our side.
    pub ours: Option<Vec<u8>>,
    /// Incoming side (stage 3 — "theirs"). `None` when the file was
    /// deleted on the incoming side.
    pub theirs: Option<Vec<u8>>,
}

/// Information about a local branch.
#[derive(Debug, Clone)]
pub struct BranchInfo {
    /// Branch name.
    pub name: String,
    /// Whether this is the currently checked-out branch.
    pub is_head: bool,
    /// Upstream tracking branch name (e.g. `"origin/main"`).
    pub upstream: Option<String>,
}

/// A file with its status.
#[derive(Debug, Clone)]
pub struct StatusEntry {
    /// Repository-relative file path.
    pub path: PathBuf,
    /// Git status.
    pub status: FileStatus,
}

/// A stash entry.
#[derive(Debug, Clone)]
pub struct StashEntry {
    /// 0-based position in the stash stack (0 = most recent).
    pub index: usize,
    /// Human-readable stash message (e.g. `"WIP on main: abc1234 …"`).
    pub message: String,
    /// Short hex hash of the stash commit.
    pub oid: String,
}

/// A git tag (annotated or lightweight).
#[derive(Debug, Clone)]
pub struct TagInfo {
    /// Short tag name (e.g. `"v1.0.0"`).
    pub name: String,
    /// Short hex hash of the tagged commit (7 chars).
    pub target_hash: String,
    /// `true` for annotated tags (have their own object + message).
    pub is_annotated: bool,
    /// Tag message (annotated tags only).
    pub message: Option<String>,
}
