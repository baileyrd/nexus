//! Git error types.

/// Errors from the git subsystem.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// The path is not inside a git repository.
    #[error("not a git repository: {0}")]
    NotARepo(String),

    /// Underlying libgit2 error.
    #[error("git error: {0}")]
    Git(#[from] git2::Error),

    /// File not found in the repository.
    #[error("file not found in repository: {0}")]
    FileNotFound(String),

    /// Working tree has uncommitted changes.
    #[error("working tree is dirty — commit or stash changes first")]
    DirtyWorkTree,

    /// Merge produced conflicts.
    #[error("merge conflicts in {0} file(s)")]
    MergeConflict(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_a_repo_display() {
        let err = GitError::NotARepo("/tmp/nope".to_string());
        assert_eq!(err.to_string(), "not a git repository: /tmp/nope");
    }

    #[test]
    fn file_not_found_display() {
        let err = GitError::FileNotFound("missing.rs".to_string());
        assert_eq!(err.to_string(), "file not found in repository: missing.rs");
    }
}
