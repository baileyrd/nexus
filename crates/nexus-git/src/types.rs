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

/// A file with its status.
#[derive(Debug, Clone)]
pub struct StatusEntry {
    /// Repository-relative file path.
    pub path: PathBuf,
    /// Git status.
    pub status: FileStatus,
}
