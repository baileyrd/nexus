//! Git engine wrapping `git2::Repository` for read and write operations.

use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};
use git2::{ApplyLocation, DiffOptions, Repository, StatusOptions};

use crate::error::GitError;
use crate::types::{GitState, RepoState, StatusEntry, FileStatus, HunkDiff, BlameEntry, LogEntry, BranchInfo, MergeResult, DiffLineKind, DiffLine, TagInfo, StashEntry};

/// Git engine backed by `git2::Repository`.
///
/// Not `Send`/`Sync` because `git2::Repository` is not thread-safe.
/// Create one per CLI invocation or TUI refresh cycle.
pub struct GitEngine {
    repo: Repository,
}

impl GitEngine {
    /// Open a git repository at the given path.
    ///
    /// Uses `Repository::open()` which looks for repository metadata
    /// at `path` itself (typically `path/.git`) and does **not**
    /// traverse parent directories. Pre-#85 this used
    /// `Repository::discover()`, which walked upward — a forge
    /// nested inside an unrelated parent git repo (e.g. a notes
    /// vault checked out under a dotfiles repo) would silently
    /// operate on the parent's history. The explicit-open shape
    /// fails fast with `NotARepo` instead.
    ///
    /// # Errors
    ///
    /// Returns [`GitError::NotARepo`] if no git repository exists at
    /// `path` (the parent dirs are no longer searched).
    pub fn open(path: &Path) -> Result<Self, GitError> {
        let repo = Repository::open(path).map_err(|_| {
            GitError::NotARepo(path.display().to_string())
        })?;
        Ok(Self { repo })
    }

    /// Return the repository working directory root.
    ///
    /// # Panics
    ///
    /// Panics if the repository is bare (no workdir).
    #[must_use] 
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
                .target().map_or_else(|| "(none)".to_string(), |oid| oid.to_string()[..7].to_string()),
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
    ///
    /// # Panics
    ///
    /// Panics if a blame hunk index is out of range (should not happen).
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
                commit_hash: commit_id.to_string()[..7].to_string(),
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
        let Ok(head) = self.repo.head() else {
            return Ok(Vec::new()); // empty repo
        };
        let Some(head_oid) = head.target() else {
            return Ok(Vec::new());
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
        let Ok(head) = self.repo.head() else {
            return Ok(Vec::new());
        };
        let Some(head_oid) = head.target() else {
            return Ok(Vec::new());
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

    /// Stage specific hunks of a file's working-tree changes.
    ///
    /// Builds a partial unified-diff patch containing only the hunks at the
    /// given 0-based indices and applies it to the index via
    /// `repo.apply(diff, ApplyLocation::Index, None)`.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure or invalid hunk index.
    pub fn stage_hunks(&self, path: &Path, hunk_indices: &[usize]) -> Result<(), GitError> {
        let hunks = self.diff_file(path)?;
        let patch = build_patch_for_hunks(path, &hunks, hunk_indices, false);
        if patch.is_empty() {
            return Ok(());
        }
        let diff = git2::Diff::from_buffer(&patch)?;
        self.repo.apply(&diff, ApplyLocation::Index, None)?;
        Ok(())
    }

    /// Unstage specific hunks of a file's staged changes.
    ///
    /// Builds a reversed partial patch containing only the hunks at the given
    /// 0-based indices and applies it to the index, effectively moving those
    /// hunks back to the working tree.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure or invalid hunk index.
    pub fn unstage_hunks(&self, path: &Path, hunk_indices: &[usize]) -> Result<(), GitError> {
        let file_diffs = self.diff_staged()?;
        let path_str = path.to_string_lossy();
        let hunks = file_diffs
            .into_iter()
            .find(|(p, _)| p == path_str.as_ref())
            .map(|(_, h)| h)
            .unwrap_or_default();
        let patch = build_patch_for_hunks(path, &hunks, hunk_indices, true);
        if patch.is_empty() {
            return Ok(());
        }
        let diff = git2::Diff::from_buffer(&patch)?;
        self.repo.apply(&diff, ApplyLocation::Index, None)?;
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

        Ok(oid.to_string()[..7].to_string())
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

    // ── Level 2: Remote Operations ─────────────────────────────────────────

    /// List configured remote names.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn remotes(&self) -> Result<Vec<String>, GitError> {
        let remotes = self.repo.remotes()?;
        Ok(remotes.iter().filter_map(|r| r.map(String::from)).collect())
    }

    /// Fetch all refs from a remote.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 or network failure.
    pub fn fetch(&self, remote_name: &str) -> Result<(), GitError> {
        let mut remote = self.repo.find_remote(remote_name)?;
        let mut fo = git2::FetchOptions::new();
        fo.remote_callbacks(make_callbacks());
        let refspecs: &[&str] = &[];
        remote.fetch(refspecs, Some(&mut fo), None)?;
        Ok(())
    }

    /// Push a branch to a remote.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 or network failure.
    pub fn push(&self, remote_name: &str, branch: &str) -> Result<(), GitError> {
        let mut remote = self.repo.find_remote(remote_name)?;
        let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
        let mut po = git2::PushOptions::new();
        po.remote_callbacks(make_callbacks());
        remote.push(&[&refspec], Some(&mut po))?;
        Ok(())
    }

    /// Push all local tags to a remote (`refs/tags/*:refs/tags/*`).
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on network failure or libgit2 error.
    pub fn push_tags(&self, remote_name: &str) -> Result<(), GitError> {
        let mut remote = self.repo.find_remote(remote_name)?;
        let mut po = git2::PushOptions::new();
        po.remote_callbacks(make_callbacks());
        remote.push(&["refs/tags/*:refs/tags/*"], Some(&mut po))?;
        Ok(())
    }

    /// List all local tags (annotated and lightweight).
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn list_tags(&self) -> Result<Vec<TagInfo>, GitError> {
        let mut tags = Vec::new();
        let refs = self.repo.references_glob("refs/tags/*")?;
        for r in refs {
            let r = r?;
            let name = r.shorthand().unwrap_or("").to_string();
            // Annotated tags: the ref's direct target OID is a tag object.
            // Lightweight tags: the ref's direct target is a commit OID.
            // `find_tag(oid)` succeeds iff the OID refers to a tag object.
            let direct_oid = r.target().unwrap_or(git2::Oid::zero());
            let (is_annotated, message) = match self.repo.find_tag(direct_oid) {
                Ok(tag_obj) => {
                    let msg = tag_obj.message().map(|m| m.trim_end_matches('\n').to_string());
                    (true, msg)
                }
                Err(_) => (false, None),
            };
            let commit = r.peel_to_commit()?;
            let target_hash = commit.id().to_string()[..7].to_string();
            tags.push(TagInfo { name, target_hash, is_annotated, message });
        }
        tags.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(tags)
    }

    /// Create a tag pointing at HEAD.
    ///
    /// If `message` is `Some`, creates an annotated tag (stores the tagger
    /// signature + message). Otherwise creates a lightweight tag (a bare
    /// ref to the commit).
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure, including if the tag
    /// already exists (`force = false`).
    pub fn create_tag(&self, name: &str, message: Option<&str>) -> Result<(), GitError> {
        let head = self.repo.head()?;
        let obj = head.peel(git2::ObjectType::Any)?;
        if let Some(msg) = message {
            let sig = self.repo.signature()?;
            self.repo.tag(name, &obj, &sig, msg, false)?;
        } else {
            self.repo.tag_lightweight(name, &obj, false)?;
        }
        Ok(())
    }

    /// Delete a local tag by name.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] if the tag does not exist or libgit2 fails.
    pub fn delete_tag(&self, name: &str) -> Result<(), GitError> {
        self.repo.tag_delete(name)?;
        Ok(())
    }

    // ── Stash ─────────────────────────────────────────────────────────────────

    /// Save the current dirty state to the stash stack.
    ///
    /// Returns the index of the new stash entry (always 0 — stash is a stack
    /// and the new entry is pushed to the top).
    ///
    /// If `message` is `None` a default `"WIP on {branch}: {head}"` message
    /// is generated. Returns an error if the working tree is clean (nothing
    /// to stash).
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn stash_push(&mut self, message: Option<&str>) -> Result<usize, GitError> {
        let sig = self.repo.signature()?;
        let default_msg;
        let msg = if let Some(m) = message {
            m
        } else {
            let state = self.state()?;
            let branch = state.branch.as_deref().unwrap_or("HEAD");
            default_msg = format!("WIP on {branch}: {}", state.head_oid);
            &default_msg
        };
        self.repo.stash_save(&sig, msg, None)?;
        Ok(0)
    }

    /// List all stash entries (newest first).
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn stash_list(&mut self) -> Result<Vec<StashEntry>, GitError> {
        let mut entries = Vec::new();
        self.repo.stash_foreach(|index, message, oid| {
            entries.push(StashEntry {
                index,
                message: message.to_string(),
                oid: oid.to_string()[..7].to_string(),
            });
            true // continue iteration
        })?;
        Ok(entries)
    }

    /// Apply a stash entry to the working tree and remove it from the stack.
    ///
    /// `index` defaults to 0 (most recent stash). After this call the stash
    /// entry is gone regardless of whether the apply had conflicts.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn stash_pop(&mut self, index: usize) -> Result<(), GitError> {
        self.repo.stash_pop(index, None)?;
        Ok(())
    }

    /// Apply a stash entry to the working tree without removing it.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn stash_apply(&mut self, index: usize) -> Result<(), GitError> {
        self.repo.stash_apply(index, None)?;
        Ok(())
    }

    /// Drop a stash entry from the stack without applying it.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn stash_drop(&mut self, index: usize) -> Result<(), GitError> {
        self.repo.stash_drop(index)?;
        Ok(())
    }

    /// Pull from a remote: fetch + merge the tracking branch.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on network failure, merge conflict, or libgit2 error.
    pub fn pull(&self, remote_name: &str, branch: &str) -> Result<MergeResult, GitError> {
        self.fetch(remote_name)?;
        let remote_ref = format!("{remote_name}/{branch}");
        self.merge(&remote_ref)
    }

    // ── Level 2: Merge Operations ───────────────────────────────────────────

    /// Merge a branch (local or remote-tracking) into HEAD.
    ///
    /// Returns a [`MergeResult`] describing the outcome.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn merge(&self, branch_name: &str) -> Result<MergeResult, GitError> {
        // Resolve the branch to an annotated commit.
        let annotated = self.repo.find_reference(&format!("refs/heads/{branch_name}"))
            .or_else(|_| self.repo.find_reference(&format!("refs/remotes/{branch_name}")))
            .and_then(|reference| self.repo.reference_to_annotated_commit(&reference))?;

        let (analysis, _preference) = self.repo.merge_analysis(&[&annotated])?;

        if analysis.is_up_to_date() {
            return Ok(MergeResult {
                fast_forward: false,
                conflicts: Vec::new(),
                commit_hash: None,
            });
        }

        if analysis.is_fast_forward() {
            // Fast-forward: just advance HEAD.
            let target_oid = annotated.id();
            let mut head_ref = self.repo.head()?;
            head_ref.set_target(target_oid, &format!("fast-forward to {branch_name}"))?;
            self.repo.checkout_head(Some(
                git2::build::CheckoutBuilder::new().force(),
            ))?;
            return Ok(MergeResult {
                fast_forward: true,
                conflicts: Vec::new(),
                commit_hash: Some(target_oid.to_string()[..7].to_string()),
            });
        }

        // Normal merge.
        self.repo.merge(&[&annotated], None, None)?;

        // Check for conflicts.
        let index = self.repo.index()?;
        if index.has_conflicts() {
            let conflicts: Vec<String> = index
                .conflicts()?
                .filter_map(std::result::Result::ok)
                .filter_map(|c| {
                    c.our
                        .as_ref()
                        .or(c.their.as_ref())
                        .and_then(|e| String::from_utf8(e.path.clone()).ok())
                })
                .collect();
            return Ok(MergeResult {
                fast_forward: false,
                conflicts,
                commit_hash: None,
            });
        }

        // No conflicts — create merge commit.
        let mut index = self.repo.index()?;
        let tree_id = index.write_tree()?;
        let tree = self.repo.find_tree(tree_id)?;
        let sig = self.repo.signature()?;
        let head_commit = self.repo.head()?.peel_to_commit()?;
        let merge_commit = self.repo.find_commit(annotated.id())?;

        let message = format!("Merge branch '{branch_name}'");
        let oid = self.repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            &message,
            &tree,
            &[&head_commit, &merge_commit],
        )?;

        self.repo.cleanup_state()?;

        Ok(MergeResult {
            fast_forward: false,
            conflicts: Vec::new(),
            commit_hash: Some(oid.to_string()[..7].to_string()),
        })
    }

    /// List files with unresolved merge conflicts.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn conflict_files(&self) -> Result<Vec<String>, GitError> {
        let index = self.repo.index()?;
        if !index.has_conflicts() {
            return Ok(Vec::new());
        }
        let files: Vec<String> = index
            .conflicts()?
            .filter_map(std::result::Result::ok)
            .filter_map(|c| {
                c.our
                    .as_ref()
                    .or(c.their.as_ref())
                    .and_then(|e| String::from_utf8(e.path.clone()).ok())
            })
            .collect();
        Ok(files)
    }

    /// Abort an in-progress merge, restoring the pre-merge state.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] on any libgit2 failure.
    pub fn abort_merge(&self) -> Result<(), GitError> {
        let head = self.repo.head()?.peel_to_commit()?;
        self.repo.reset(head.as_object(), git2::ResetType::Hard, None)?;
        self.repo.cleanup_state()?;
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

fn make_callbacks() -> git2::RemoteCallbacks<'static> {
    let mut callbacks = git2::RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, allowed_types| {
        let username = username_from_url.unwrap_or("git");
        // Try SSH agent first.
        if allowed_types.contains(git2::CredentialType::SSH_KEY) {
            if let Ok(cred) = git2::Cred::ssh_key_from_agent(username) {
                return Ok(cred);
            }
            // Try each default key, optionally using a cached passphrase.
            // BL-090: passphrases are looked up from the OS keyring under
            // `ssh-passphrase:<key_name>` so encrypted keys work without
            // ssh-agent. The vault is cheap to construct (`disabled: bool`),
            // so creating one per credential probe is fine.
            if let Some(home) = std::env::var_os("HOME") {
                let home = std::path::PathBuf::from(home);
                let vault = nexus_security::CredentialVault::new();
                for key_name in &["id_ed25519", "id_rsa"] {
                    let key_path = home.join(".ssh").join(key_name);
                    if !key_path.exists() {
                        continue;
                    }
                    // 1. Unencrypted key (no passphrase needed).
                    if let Ok(cred) = git2::Cred::ssh_key(username, None, &key_path, None) {
                        return Ok(cred);
                    }
                    // 2. Cached passphrase from OS keyring.
                    if let Ok(passphrase) =
                        vault.retrieve(&format!("ssh-passphrase:{key_name}"))
                    {
                        if let Ok(cred) =
                            git2::Cred::ssh_key(username, None, &key_path, Some(&passphrase))
                        {
                            return Ok(cred);
                        }
                    }
                }
            }
        }
        // Try default credentials (for HTTPS).
        if allowed_types.contains(git2::CredentialType::DEFAULT) {
            return git2::Cred::default();
        }
        Err(git2::Error::from_str("no credentials available"))
    });
    callbacks
}

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
        hash: commit.id().to_string()[..7].to_string(),
        author: String::from_utf8_lossy(sig.name_bytes()).to_string(),
        date: git_time_to_chrono(&sig.when()),
        message: commit.message().unwrap_or("").to_string(),
        parents: commit
            .parent_ids()
            .map(|id| id.to_string()[..7].to_string())
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

        let diff_patch = git2::Patch::from_diff(diff, delta_idx)?;
        if let Some(diff_patch) = diff_patch {
            let mut hunks = Vec::new();
            for hunk_idx in 0..diff_patch.num_hunks() {
                let (hunk, _count) = diff_patch.hunk(hunk_idx)?;
                let mut lines = Vec::new();
                for line_idx in 0..diff_patch.num_lines_in_hunk(hunk_idx)? {
                    let line = diff_patch.line_in_hunk(hunk_idx, line_idx)?;
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

/// Build a minimal unified-diff patch containing only the selected hunks.
///
/// When `reverse` is true the `+`/`-` prefixes are swapped and the `@@`
/// header old/new counts are transposed so the patch removes what the
/// original added (used for `unstage_hunks`).
///
/// The caller must ensure hunk indices are valid; out-of-range entries are
/// silently skipped.
fn build_patch_for_hunks(
    path: &Path,
    all_hunks: &[HunkDiff],
    indices: &[usize],
    reverse: bool,
) -> Vec<u8> {
    // Collect only the valid, in-range indices up front.
    let selected: Vec<&HunkDiff> = indices
        .iter()
        .filter_map(|&i| all_hunks.get(i))
        .collect();
    if selected.is_empty() {
        return Vec::new();
    }

    let path_str = path.to_string_lossy();
    // libgit2's Diff::from_buffer requires the full git diff header.
    let mut out = format!(
        "diff --git a/{path_str} b/{path_str}\n--- a/{path_str}\n+++ b/{path_str}\n"
    );

    for hunk in selected {
        let (os, oc, ns, nc) = if reverse {
            (hunk.new_start, hunk.new_count, hunk.old_start, hunk.old_count)
        } else {
            (hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count)
        };

        out.push_str(&format!("@@ -{os},{oc} +{ns},{nc} @@\n"));

        for line in &hunk.lines {
            let prefix = match (reverse, &line.kind) {
                (false, DiffLineKind::Added)   => '+',
                (false, DiffLineKind::Removed) => '-',
                (true,  DiffLineKind::Added)   => '-',
                (true,  DiffLineKind::Removed) => '+',
                _                              => ' ',
            };
            // Normalize: strip any trailing newline then re-add exactly one.
            // libgit2 includes the line terminator in DiffLine::content; the
            // Nexus DiffLine type documents it as absent — normalising handles
            // both cases safely.
            let content = line.content.trim_end_matches(['\n', '\r']);
            out.push(prefix);
            out.push_str(content);
            out.push('\n');
        }
    }

    out.into_bytes()
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
    fn stash_push_and_list() {
        let (dir, mut engine) = init_repo();
        fs::write(dir.path().join("file.txt"), "original").unwrap();
        make_commit(&engine, "initial");

        // Modify the file to make the tree dirty.
        fs::write(dir.path().join("file.txt"), "dirty").unwrap();
        assert!(engine.state().unwrap().is_dirty);

        // Stash.
        let idx = engine.stash_push(Some("my stash")).unwrap();
        assert_eq!(idx, 0);

        // Working tree should be clean after stash.
        assert!(!engine.state().unwrap().is_dirty);

        // List should have one entry.
        let entries = engine.stash_list().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].index, 0);
        assert!(entries[0].message.contains("my stash"));
    }

    #[test]
    fn stash_pop_restores_changes() {
        let (dir, mut engine) = init_repo();
        fs::write(dir.path().join("file.txt"), "original").unwrap();
        make_commit(&engine, "initial");

        fs::write(dir.path().join("file.txt"), "dirty").unwrap();
        engine.stash_push(None).unwrap();
        assert!(!engine.state().unwrap().is_dirty);

        engine.stash_pop(0).unwrap();
        assert!(engine.state().unwrap().is_dirty);

        // Stash should be empty after pop.
        assert!(engine.stash_list().unwrap().is_empty());
    }

    #[test]
    fn stash_drop_discards_entry() {
        let (dir, mut engine) = init_repo();
        fs::write(dir.path().join("file.txt"), "original").unwrap();
        make_commit(&engine, "initial");

        fs::write(dir.path().join("file.txt"), "dirty").unwrap();
        engine.stash_push(None).unwrap();

        engine.stash_drop(0).unwrap();
        // Still clean — drop doesn't apply.
        assert!(!engine.state().unwrap().is_dirty);
        assert!(engine.stash_list().unwrap().is_empty());
    }

    #[test]
    fn list_tags_empty_repo() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("file.txt"), "content").unwrap();
        make_commit(&engine, "initial");
        let tags = engine.list_tags().unwrap();
        assert!(tags.is_empty());
    }

    #[test]
    fn create_lightweight_tag_and_list() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("file.txt"), "content").unwrap();
        make_commit(&engine, "initial");

        engine.create_tag("v1.0.0", None).unwrap();
        let tags = engine.list_tags().unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "v1.0.0");
        assert!(!tags[0].is_annotated);
        assert!(tags[0].message.is_none());
    }

    #[test]
    fn create_annotated_tag_and_list() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("file.txt"), "content").unwrap();
        make_commit(&engine, "initial");

        engine.create_tag("v2.0.0", Some("Release 2.0")).unwrap();
        let tags = engine.list_tags().unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "v2.0.0");
        assert!(tags[0].is_annotated);
        assert_eq!(tags[0].message.as_deref(), Some("Release 2.0"));
    }

    #[test]
    fn delete_tag_removes_it() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("file.txt"), "content").unwrap();
        make_commit(&engine, "initial");

        engine.create_tag("v1.0.0", None).unwrap();
        assert_eq!(engine.list_tags().unwrap().len(), 1);

        engine.delete_tag("v1.0.0").unwrap();
        assert!(engine.list_tags().unwrap().is_empty());
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

    #[test]
    fn stage_hunks_stages_only_selected_hunk() {
        let (dir, engine) = init_repo();
        // Create a file with two separate sections so the diff has two hunks.
        fs::write(
            dir.path().join("file.txt"),
            "line1\nline2\nline3\n\n\n\n\nline8\nline9\nline10\n",
        )
        .unwrap();
        make_commit(&engine, "initial");

        // Modify both sections.
        fs::write(
            dir.path().join("file.txt"),
            "CHANGED1\nline2\nline3\n\n\n\n\nline8\nCHANGED9\nline10\n",
        )
        .unwrap();

        let path = Path::new("file.txt");
        let hunks = engine.diff_file(path).unwrap();
        // Expect 2 hunks (one per modified section).
        assert_eq!(hunks.len(), 2, "expected 2 hunks, got {}", hunks.len());

        // Stage only hunk 0.
        engine.stage_hunks(path, &[0]).unwrap();

        // The file should now be in the staged diff.
        let staged = engine.diff_staged().unwrap();
        assert!(!staged.is_empty(), "expected staged diff after stage_hunks");
        // And the working-tree diff should still have hunk 1 unstaged.
        let remaining = engine.diff_file(path).unwrap();
        assert!(!remaining.is_empty(), "expected remaining unstaged hunk");
    }

    #[test]
    fn unstage_hunks_removes_selected_hunk_from_index() {
        let (dir, engine) = init_repo();
        fs::write(dir.path().join("file.txt"), "original\n").unwrap();
        make_commit(&engine, "initial");

        // Stage the whole file.
        fs::write(dir.path().join("file.txt"), "modified\n").unwrap();
        engine.stage_file(Path::new("file.txt")).unwrap();

        let staged = engine.diff_staged().unwrap();
        assert!(!staged.is_empty());

        // Unstage hunk 0.
        engine.unstage_hunks(Path::new("file.txt"), &[0]).unwrap();

        // Index should now match HEAD again (no staged diff).
        let after = engine.diff_staged().unwrap();
        assert!(
            after.is_empty() || after.iter().all(|(_, hs)| hs.is_empty()),
            "expected empty staged diff after unstage_hunks"
        );
    }
}
