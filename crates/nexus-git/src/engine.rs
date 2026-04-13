//! Git engine wrapping `git2::Repository` for read and write operations.

use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};
use git2::{DiffOptions, Repository, StatusOptions};

use crate::error::GitError;
use crate::types::*;

/// Git engine backed by `git2::Repository`.
///
/// Not `Send`/`Sync` because `git2::Repository` is not thread-safe.
/// Create one per CLI invocation or TUI refresh cycle.
pub struct GitEngine {
    repo: Repository,
}

impl GitEngine {
    /// Discover and open a git repository from the given path.
    ///
    /// Uses `Repository::discover()` to find the `.git` directory
    /// by traversing parent directories.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::NotARepo`] if no git repository is found.
    pub fn open(path: &Path) -> Result<Self, GitError> {
        let repo = Repository::discover(path).map_err(|_| {
            GitError::NotARepo(path.display().to_string())
        })?;
        Ok(Self { repo })
    }

    /// Return the repository working directory root.
    ///
    /// # Panics
    ///
    /// Panics if the repository is bare (no workdir).
    pub fn repo_root(&self) -> &Path {
        self.repo.workdir().expect("bare repos not supported")
    }

    /// Query the current repository state.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn state(&self) -> Result<GitState, GitError> {
        let branch = match self.repo.head() {
            Ok(head) => head.shorthand().map(String::from),
            Err(_) => None, // empty repo or detached HEAD
        };

        let head_oid = match self.repo.head() {
            Ok(head) => head
                .target()
                .map(|oid| format!("{}", &oid.to_string()[..7]))
                .unwrap_or_else(|| "(none)".to_string()),
            Err(_) => "(none)".to_string(),
        };

        let is_dirty = {
            let mut opts = StatusOptions::new();
            opts.include_untracked(true)
                .recurse_untracked_dirs(false);
            match self.repo.statuses(Some(&mut opts)) {
                Ok(statuses) => !statuses.is_empty(),
                Err(_) => false,
            }
        };

        let repo_state = match self.repo.state() {
            git2::RepositoryState::Clean => RepoState::Clean,
            git2::RepositoryState::Merge => RepoState::Merge,
            git2::RepositoryState::Rebase
            | git2::RepositoryState::RebaseMerge => RepoState::Rebase,
            git2::RepositoryState::RebaseInteractive => RepoState::RebaseInteractive,
            git2::RepositoryState::CherryPick
            | git2::RepositoryState::CherryPickSequence => RepoState::CherryPick,
            git2::RepositoryState::Revert
            | git2::RepositoryState::RevertSequence => RepoState::Revert,
            git2::RepositoryState::Bisect => RepoState::Bisect,
            _ => RepoState::Clean,
        };

        Ok(GitState {
            branch,
            head_oid,
            is_dirty,
            repo_state,
        })
    }

    /// List all files with non-clean status.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn file_statuses(&self) -> Result<Vec<StatusEntry>, GitError> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_unmodified(false);

        let statuses = self.repo.statuses(Some(&mut opts))?;
        let mut result = Vec::new();

        for entry in statuses.iter() {
            let path = entry.path().unwrap_or("").to_string();
            let status = map_status(entry.status());
            result.push(StatusEntry {
                path: PathBuf::from(path),
                status,
            });
        }

        Ok(result)
    }

    /// Query the status of a single file.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn file_status(&self, path: &Path) -> Result<FileStatus, GitError> {
        let status = self.repo.status_file(path)?;
        Ok(map_status(status))
    }

    /// Compute the diff between HEAD and the working tree for a single file.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn diff_file(&self, path: &Path) -> Result<Vec<HunkDiff>, GitError> {
        let head_tree = self.head_tree()?;

        let mut opts = DiffOptions::new();
        opts.pathspec(path.to_string_lossy().as_ref());

        let diff = self.repo.diff_tree_to_workdir_with_index(
            head_tree.as_ref(),
            Some(&mut opts),
        )?;

        collect_hunks(&diff)
    }

    /// Compute the diff of staged changes (index vs HEAD).
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn diff_staged(&self) -> Result<Vec<(String, Vec<HunkDiff>)>, GitError> {
        let head_tree = self.head_tree()?;

        let diff = self.repo.diff_tree_to_index(
            head_tree.as_ref(),
            None,
            None,
        )?;

        collect_file_hunks(&diff)
    }

    /// Compute blame annotations for a file.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn blame(&self, path: &Path) -> Result<Vec<BlameEntry>, GitError> {
        let blame = self.repo.blame_file(path, None)?;
        let mut entries = Vec::new();

        for i in 0..blame.len() {
            let hunk = blame.get_index(i).expect("blame hunk in range");
            let sig = hunk.final_signature();
            let commit_id = hunk.final_commit_id();

            let message = self
                .repo
                .find_commit(commit_id)
                .ok()
                .and_then(|c| c.summary().map(String::from))
                .unwrap_or_default();

            let date = git_time_to_chrono(&sig.when());

            entries.push(BlameEntry {
                commit_hash: format!("{}", &commit_id.to_string()[..7]),
                author: String::from_utf8_lossy(sig.name_bytes()).to_string(),
                date,
                message,
                start_line: hunk.final_start_line(),
                end_line: hunk.final_start_line() + hunk.lines_in_hunk() - 1,
            });
        }

        Ok(entries)
    }

    /// Walk the commit log from HEAD, returning up to `limit` entries.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn log(&self, limit: usize) -> Result<Vec<LogEntry>, GitError> {
        let head = match self.repo.head() {
            Ok(h) => h,
            Err(_) => return Ok(Vec::new()), // empty repo
        };
        let head_oid = match head.target() {
            Some(oid) => oid,
            None => return Ok(Vec::new()),
        };

        let mut revwalk = self.repo.revwalk()?;
        revwalk.push(head_oid)?;
        revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;

        let mut entries = Vec::new();
        for oid in revwalk.take(limit) {
            let oid = oid?;
            let commit = self.repo.find_commit(oid)?;
            entries.push(commit_to_log_entry(&commit));
        }

        Ok(entries)
    }

    /// Walk the commit log for a specific file, returning up to `limit` entries.
    ///
    /// Only includes commits where the file's blob changed compared to the parent.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn log_file(&self, path: &Path, limit: usize) -> Result<Vec<LogEntry>, GitError> {
        let head = match self.repo.head() {
            Ok(h) => h,
            Err(_) => return Ok(Vec::new()),
        };
        let head_oid = match head.target() {
            Some(oid) => oid,
            None => return Ok(Vec::new()),
        };

        let mut revwalk = self.repo.revwalk()?;
        revwalk.push(head_oid)?;
        revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;

        let path_str = path.to_string_lossy();
        let mut entries = Vec::new();

        for oid in revwalk {
            if entries.len() >= limit {
                break;
            }
            let oid = oid?;
            let commit = self.repo.find_commit(oid)?;

            // Check if this commit changed the file vs its first parent.
            let dominated = if commit.parent_count() == 0 {
                // Root commit — file must be new if it exists in the tree.
                commit
                    .tree()
                    .ok()
                    .and_then(|t| t.get_path(path).ok())
                    .is_some()
            } else {
                let parent = commit.parent(0)?;
                let mut opts = DiffOptions::new();
                opts.pathspec(path_str.as_ref());
                let diff = self.repo.diff_tree_to_tree(
                    Some(&parent.tree()?),
                    Some(&commit.tree()?),
                    Some(&mut opts),
                )?;
                diff.stats()?.files_changed() > 0
            };

            if dominated {
                entries.push(commit_to_log_entry(&commit));
            }
        }

        Ok(entries)
    }

    // ── Level 2: Write Operations ──────────────────────────────────────────

    /// Stage a single file by path (repo-relative).
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn stage_file(&self, path: &Path) -> Result<(), GitError> {
        let mut index = self.repo.index()?;
        // If the file was deleted from disk, remove from index; otherwise add.
        let abs = self.repo_root().join(path);
        if abs.exists() {
            index.add_path(path)?;
        } else {
            index.remove_path(path)?;
        }
        index.write()?;
        Ok(())
    }

    /// Stage all changes (tracked modifications and untracked files).
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn stage_all(&self) -> Result<(), GitError> {
        let mut index = self.repo.index()?;
        index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
        index.write()?;
        Ok(())
    }

    /// Unstage a single file (reset its index entry to HEAD).
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn unstage_file(&self, path: &Path) -> Result<(), GitError> {
        let head = self.repo.head()?;
        let commit = head.peel_to_commit()?;
        self.repo.reset_default(Some(commit.as_object()), [path])?;
        Ok(())
    }

    /// Unstage all files (reset the index to match HEAD).
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn unstage_all(&self) -> Result<(), GitError> {
        let head = self.repo.head()?;
        let commit = head.peel_to_commit()?;
        self.repo.reset(
            commit.as_object(),
            git2::ResetType::Mixed,
            None,
        )?;
        Ok(())
    }

    /// Create a commit from the current index with the given message.
    ///
    /// Returns the short hex hash of the new commit.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn commit(&self, message: &str) -> Result<String, GitError> {
        let mut index = self.repo.index()?;
        let tree_id = index.write_tree()?;
        let tree = self.repo.find_tree(tree_id)?;
        let sig = self.repo.signature()?;

        let parents: Vec<git2::Commit<'_>> = match self.repo.head() {
            Ok(head) => vec![head.peel_to_commit()?],
            Err(_) => vec![],
        };
        let parent_refs: Vec<&git2::Commit<'_>> = parents.iter().collect();

        let oid = self.repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            message,
            &tree,
            &parent_refs,
        )?;

        Ok(format!("{}", &oid.to_string()[..7]))
    }

    /// List local branches.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn branches(&self) -> Result<Vec<BranchInfo>, GitError> {
        let head_name = self
            .repo
            .head()
            .ok()
            .and_then(|h| h.shorthand().map(String::from));

        let mut result = Vec::new();
        for branch in self.repo.branches(Some(git2::BranchType::Local))? {
            let (branch, _) = branch?;
            let name = branch
                .name()?
                .unwrap_or("")
                .to_string();
            let is_head = head_name.as_deref() == Some(&name);
            let upstream = branch
                .upstream()
                .ok()
                .and_then(|u| u.name().ok().flatten().map(String::from));

            result.push(BranchInfo {
                name,
                is_head,
                upstream,
            });
        }
        Ok(result)
    }

    /// Create a new branch from HEAD.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn create_branch(&self, name: &str) -> Result<(), GitError> {
        let head = self.repo.head()?;
        let commit = head.peel_to_commit()?;
        self.repo.branch(name, &commit, false)?;
        Ok(())
    }

    /// Switch to an existing branch.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] if the branch doesn't exist, or on libgit2 failure.
    /// Does NOT check for dirty working tree — caller should check `state().is_dirty` first.
    pub fn switch_branch(&self, name: &str) -> Result<(), GitError> {
        let refname = format!("refs/heads/{name}");
        self.repo.set_head(&refname)?;
        self.repo.checkout_head(Some(
            git2::build::CheckoutBuilder::new().force(),
        ))?;
        Ok(())
    }

    /// Delete a local branch.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] if the branch is the current HEAD or doesn't exist.
    pub fn delete_branch(&self, name: &str) -> Result<(), GitError> {
        let mut branch = self.repo.find_branch(name, git2::BranchType::Local)?;
        if branch.is_head() {
            return Err(GitError::Git(git2::Error::from_str(
                "cannot delete the currently checked-out branch",
            )));
        }
        branch.delete()?;
        Ok(())
    }

    /// Get the HEAD tree, or `None` for an empty repo.
    fn head_tree(&self) -> Result<Option<git2::Tree<'_>>, GitError> {
        match self.repo.head() {
            Ok(head) => {
                let commit = head.peel_to_commit()?;
                Ok(Some(commit.tree()?))
            }
            Err(_) => Ok(None),
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn map_status(s: git2::Status) -> FileStatus {
    if s.is_conflicted() {
        FileStatus::Conflicted
    } else if s.is_index_new() {
        FileStatus::Added
    } else if s.is_index_modified() || s.is_index_renamed() {
        FileStatus::Staged
    } else if s.is_index_deleted() {
        FileStatus::Removed
    } else if s.is_wt_modified() {
        FileStatus::Modified
    } else if s.is_wt_deleted() {
        FileStatus::Removed
    } else if s.is_wt_new() {
        FileStatus::Untracked
    } else if s.is_wt_renamed() {
        FileStatus::Renamed
    } else {
        FileStatus::Unmodified
    }
}

fn git_time_to_chrono(time: &git2::Time) -> DateTime<Utc> {
    Utc.timestamp_opt(time.seconds(), 0)
        .single()
        .unwrap_or_default()
}

fn commit_to_log_entry(commit: &git2::Commit<'_>) -> LogEntry {
    let sig = commit.author();
    LogEntry {
        hash: format!("{}", &commit.id().to_string()[..7]),
        author: String::from_utf8_lossy(sig.name_bytes()).to_string(),
        date: git_time_to_chrono(&sig.when()),
        message: commit.message().unwrap_or("").to_string(),
        parents: commit
            .parent_ids()
            .map(|id| format!("{}", &id.to_string()[..7]))
            .collect(),
    }
}

fn collect_hunks(diff: &git2::Diff<'_>) -> Result<Vec<HunkDiff>, GitError> {
    let mut all_hunks = Vec::new();
    for delta_idx in 0..diff.deltas().len() {
        let patch = git2::Patch::from_diff(diff, delta_idx)?;
        if let Some(patch) = patch {
            for hunk_idx in 0..patch.num_hunks() {
                let (hunk, _count) = patch.hunk(hunk_idx)?;
                let mut lines = Vec::new();
                for line_idx in 0..patch.num_lines_in_hunk(hunk_idx)? {
                    let line = patch.line_in_hunk(hunk_idx, line_idx)?;
                    let kind = match line.origin() {
                        '+' => DiffLineKind::Added,
                        '-' => DiffLineKind::Removed,
                        _ => DiffLineKind::Context,
                    };
                    let content = String::from_utf8_lossy(line.content()).to_string();
                    lines.push(DiffLine { kind, content });
                }
                all_hunks.push(HunkDiff {
                    old_start: hunk.old_start(),
                    old_count: hunk.old_lines(),
                    new_start: hunk.new_start(),
                    new_count: hunk.new_lines(),
                    lines,
                });
            }
        }
    }
    Ok(all_hunks)
}

fn collect_file_hunks(diff: &git2::Diff<'_>) -> Result<Vec<(String, Vec<HunkDiff>)>, GitError> {
    // Process each delta (file) individually to avoid borrow conflicts.
    let mut files = Vec::new();

    for delta_idx in 0..diff.deltas().len() {
        let delta = diff.deltas().nth(delta_idx).expect("delta in range");
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let patch = git2::Patch::from_diff(diff, delta_idx)?;
        if let Some(patch) = patch {
            let mut hunks = Vec::new();
            for hunk_idx in 0..patch.num_hunks() {
                let (hunk, _count) = patch.hunk(hunk_idx)?;
                let mut lines = Vec::new();
                for line_idx in 0..patch.num_lines_in_hunk(hunk_idx)? {
                    let line = patch.line_in_hunk(hunk_idx, line_idx)?;
                    let kind = match line.origin() {
                        '+' => DiffLineKind::Added,
                        '-' => DiffLineKind::Removed,
                        _ => DiffLineKind::Context,
                    };
                    let content = String::from_utf8_lossy(line.content()).to_string();
                    lines.push(DiffLine { kind, content });
                }
                hunks.push(HunkDiff {
                    old_start: hunk.old_start(),
                    old_count: hunk.old_lines(),
                    new_start: hunk.new_start(),
                    new_count: hunk.new_lines(),
                    lines,
                });
            }
            files.push((path, hunks));
        }
    }

    Ok(files)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn init_repo() -> (tempfile::TempDir, GitEngine) {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();

        // Configure user for commits.
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test User").unwrap();
        config.set_str("user.email", "test@example.com").unwrap();

        let engine = GitEngine { repo };
        (dir, engine)
    }

    fn make_commit(engine: &GitEngine, message: &str) {
        let mut index = engine.repo.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = engine.repo.find_tree(tree_id).unwrap();
        let sig = engine.repo.signature().unwrap();

        let parents: Vec<git2::Commit<'_>> = match engine.repo.head() {
            Ok(head) => vec![head.peel_to_commit().unwrap()],
            Err(_) => vec![],
        };
        let parent_refs: Vec<&git2::Commit<'_>> = parents.iter().collect();

        engine
            .repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
            .unwrap();
    }

    #[test]
    fn open_non_repo_errors() {
        let dir = tempfile::tempdir().unwrap();
        let result = GitEngine::open(dir.path());
        assert!(matches!(result, Err(GitError::NotARepo(_))));
    }

    #[test]
    fn open_valid_repo() {
        let (dir, _engine) = init_repo();
        let engine = GitEngine::open(dir.path());
        assert!(engine.is_ok());
    }

    #[test]
    fn state_fresh_repo_no_commits() {
        let (_dir, engine) = init_repo();
        let state = engine.state().unwrap();
        // No commits yet — branch might be None or "master"/"main" depending on git config.
        assert_eq!(state.head_oid, "(none)");
        assert_eq!(state.repo_state, RepoState::Clean);
    }

    #[test]
    fn state_after_commit() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("hello.md"), "# Hello\n").unwrap();
        make_commit(&engine, "initial commit");

        let state = engine.state().unwrap();
        assert!(state.branch.is_some());
        assert_ne!(state.head_oid, "(none)");
        assert!(!state.is_dirty);
        assert_eq!(state.repo_state, RepoState::Clean);
    }

    #[test]
    fn file_statuses_untracked() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("new.txt"), "content").unwrap();

        let statuses = engine.file_statuses().unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].status, FileStatus::Untracked);
    }

    #[test]
    fn file_statuses_modified() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("file.txt"), "original").unwrap();
        make_commit(&engine, "add file");

        fs::write(dir.path().join("file.txt"), "modified").unwrap();
        let statuses = engine.file_statuses().unwrap();
        assert!(
            statuses.iter().any(|s| s.status == FileStatus::Modified),
            "expected Modified status, got: {statuses:?}"
        );
    }

    #[test]
    fn diff_file_shows_changes() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("file.txt"), "line1\nline2\n").unwrap();
        make_commit(&engine, "add file");

        fs::write(dir.path().join("file.txt"), "line1\nmodified\n").unwrap();
        let hunks = engine.diff_file(Path::new("file.txt")).unwrap();
        assert!(!hunks.is_empty(), "expected at least one hunk");

        let has_added = hunks.iter().any(|h| {
            h.lines.iter().any(|l| l.kind == DiffLineKind::Added)
        });
        let has_removed = hunks.iter().any(|h| {
            h.lines.iter().any(|l| l.kind == DiffLineKind::Removed)
        });
        assert!(has_added, "expected added lines in diff");
        assert!(has_removed, "expected removed lines in diff");
    }

    #[test]
    fn blame_returns_entries() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("file.txt"), "line1\nline2\nline3\n").unwrap();
        make_commit(&engine, "add file");

        let entries = engine.blame(Path::new("file.txt")).unwrap();
        assert!(!entries.is_empty());
        assert_eq!(entries[0].author, "Test User");
    }

    #[test]
    fn log_returns_commits() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("a.txt"), "a").unwrap();
        make_commit(&engine, "first");
        fs::write(dir.path().join("b.txt"), "b").unwrap();
        make_commit(&engine, "second");
        fs::write(dir.path().join("c.txt"), "c").unwrap();
        make_commit(&engine, "third");

        let log = engine.log(10).unwrap();
        assert_eq!(log.len(), 3);
        // Most recent first.
        assert!(log[0].message.contains("third"));
        assert!(log[2].message.contains("first"));
    }

    #[test]
    fn log_file_filters_to_file() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("a.txt"), "a").unwrap();
        make_commit(&engine, "add a");
        fs::write(dir.path().join("b.txt"), "b").unwrap();
        make_commit(&engine, "add b");
        fs::write(dir.path().join("a.txt"), "a modified").unwrap();
        make_commit(&engine, "modify a");

        let log_a = engine.log_file(Path::new("a.txt"), 10).unwrap();
        assert_eq!(log_a.len(), 2, "a.txt should appear in 2 commits");

        let log_b = engine.log_file(Path::new("b.txt"), 10).unwrap();
        assert_eq!(log_b.len(), 1, "b.txt should appear in 1 commit");
    }

    #[test]
    fn log_empty_repo() {
        let (_dir, engine) = init_repo();
        let log = engine.log(10).unwrap();
        assert!(log.is_empty());
    }

    #[test]
    fn state_dirty_after_modification() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("file.txt"), "content").unwrap();
        make_commit(&engine, "initial");

        // Clean.
        assert!(!engine.state().unwrap().is_dirty);

        // Dirty.
        fs::write(dir.path().join("file.txt"), "changed").unwrap();
        assert!(engine.state().unwrap().is_dirty);
    }

    // ── Level 2 tests ────────────────────────────────────────────────────────

    #[test]
    fn stage_file_marks_as_staged() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("new.txt"), "content").unwrap();

        // Before staging: untracked.
        let s = engine.file_status(Path::new("new.txt")).unwrap();
        assert_eq!(s, FileStatus::Untracked);

        // After staging: added.
        engine.stage_file(Path::new("new.txt")).unwrap();
        let s = engine.file_status(Path::new("new.txt")).unwrap();
        assert_eq!(s, FileStatus::Added);
    }

    #[test]
    fn stage_all_stages_everything() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("a.txt"), "a").unwrap();
        fs::write(dir.path().join("b.txt"), "b").unwrap();

        engine.stage_all().unwrap();
        let statuses = engine.file_statuses().unwrap();
        assert!(
            statuses.iter().all(|s| s.status == FileStatus::Added),
            "all files should be staged (Added), got: {statuses:?}"
        );
    }

    #[test]
    fn unstage_file_reverts_staging() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("file.txt"), "content").unwrap();
        make_commit(&engine, "initial");

        fs::write(dir.path().join("file.txt"), "changed").unwrap();
        engine.stage_file(Path::new("file.txt")).unwrap();
        assert_eq!(
            engine.file_status(Path::new("file.txt")).unwrap(),
            FileStatus::Staged,
        );

        engine.unstage_file(Path::new("file.txt")).unwrap();
        assert_eq!(
            engine.file_status(Path::new("file.txt")).unwrap(),
            FileStatus::Modified,
        );
    }

    #[test]
    fn commit_creates_log_entry() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("file.txt"), "content").unwrap();
        engine.stage_all().unwrap();
        let hash = engine.commit("test commit").unwrap();

        assert_eq!(hash.len(), 7);
        let log = engine.log(10).unwrap();
        assert_eq!(log.len(), 1);
        assert!(log[0].message.contains("test commit"));
        assert!(!engine.state().unwrap().is_dirty);
    }

    #[test]
    fn commit_initial_and_followup() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("a.txt"), "a").unwrap();
        engine.stage_all().unwrap();
        engine.commit("first").unwrap();

        fs::write(dir.path().join("b.txt"), "b").unwrap();
        engine.stage_all().unwrap();
        engine.commit("second").unwrap();

        let log = engine.log(10).unwrap();
        assert_eq!(log.len(), 2);
    }

    #[test]
    fn create_and_list_branches() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("file.txt"), "content").unwrap();
        make_commit(&engine, "initial");

        engine.create_branch("feature-a").unwrap();
        let branches = engine.branches().unwrap();
        let names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();
        assert!(names.contains(&"feature-a"), "feature-a should be in branches: {names:?}");
    }

    #[test]
    fn switch_branch_changes_head() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("file.txt"), "content").unwrap();
        make_commit(&engine, "initial");

        engine.create_branch("dev").unwrap();
        engine.switch_branch("dev").unwrap();

        let state = engine.state().unwrap();
        assert_eq!(state.branch.as_deref(), Some("dev"));
    }

    #[test]
    fn delete_branch_removes_it() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("file.txt"), "content").unwrap();
        make_commit(&engine, "initial");

        engine.create_branch("to-delete").unwrap();
        assert!(engine.branches().unwrap().iter().any(|b| b.name == "to-delete"));

        engine.delete_branch("to-delete").unwrap();
        assert!(!engine.branches().unwrap().iter().any(|b| b.name == "to-delete"));
    }

    #[test]
    fn delete_current_branch_fails() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("file.txt"), "content").unwrap();
        make_commit(&engine, "initial");

        let branch_name = engine.state().unwrap().branch.unwrap();
        let result = engine.delete_branch(&branch_name);
        assert!(result.is_err(), "deleting current branch should fail");
    }
}
