//! Core plugin for the git subsystem (`com.nexus.git`).
//!
//! Wraps the thread-confined [`GitWorker`] behind the kernel IPC surface so
//! any other plugin can query git state without linking `libgit2` directly.
//! Also runs a background poller that publishes `Custom` bus events whenever
//! HEAD, branch, or dirty state changes.
//!
//! # Events published
//! | type_id | when |
//! |---------|------|
//! | `com.nexus.git.state` | initial snapshot on first poll |
//! | `com.nexus.git.branch_changed` | branch switched |
//! | `com.nexus.git.commit` | HEAD hash changed |
//! | `com.nexus.git.dirty_changed` | working-tree dirty flag toggled |

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use nexus_kernel::{EventBus, EventFilter};
use nexus_plugins::{CorePlugin, PluginError};
use serde::Deserialize;
use serde_json::json;

use crate::{AutoCommitter, GitError, GitState, GitWorker, GitWorkerHandle};

// ── Auto-commit config ────────────────────────────────────────────────────────

/// Minimal subset of `app.toml` for reading auto-commit settings.
/// Mirrors `nexus_formats::config::GitSettings`; duplicated here to keep
/// `nexus-git` free of the formats crate dependency.
#[derive(Deserialize, Default)]
struct AutoCommitAppConfig {
    #[serde(default)]
    git: AutoCommitGitSettings,
}

#[derive(Deserialize)]
struct AutoCommitGitSettings {
    #[serde(default)]
    auto_commit: bool,
    #[serde(default = "default_interval")]
    auto_commit_interval_secs: u64,
    /// P2-06 — interval between `git status` polls (background poller).
    /// `None` ⇒ [`DEFAULT_POLL_INTERVAL`].
    #[serde(default)]
    poll_interval_secs: Option<u64>,
    /// P2-06 — wake-up cadence inside the auto-commit idle loop.
    /// `None` ⇒ [`DEFAULT_AUTO_COMMIT_TICK`].
    #[serde(default)]
    auto_commit_tick_secs: Option<u64>,
}

fn default_interval() -> u64 {
    1800
}

impl Default for AutoCommitGitSettings {
    fn default() -> Self {
        Self {
            auto_commit: false,
            auto_commit_interval_secs: default_interval(),
            poll_interval_secs: None,
            auto_commit_tick_secs: None,
        }
    }
}

/// P2-06 — resolved git timing knobs, sourced from `[git]` in
/// `app.toml`. `poll_interval` falls back to [`DEFAULT_POLL_INTERVAL`]
/// (2 s); `auto_commit_tick` falls back to
/// [`DEFAULT_AUTO_COMMIT_TICK`] (30 s).
struct GitTiming {
    auto_commit: bool,
    auto_commit_interval_secs: u64,
    poll_interval: Duration,
    auto_commit_tick: Duration,
}

fn read_git_settings(forge_root: &Path) -> GitTiming {
    let path = forge_root.join(".forge").join("app.toml");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let cfg: AutoCommitAppConfig = toml::from_str(&text).unwrap_or_default();
    GitTiming {
        auto_commit: cfg.git.auto_commit,
        auto_commit_interval_secs: cfg.git.auto_commit_interval_secs,
        poll_interval: cfg
            .git
            .poll_interval_secs
            .map(Duration::from_secs)
            .unwrap_or(DEFAULT_POLL_INTERVAL),
        auto_commit_tick: cfg
            .git
            .auto_commit_tick_secs
            .map(Duration::from_secs)
            .unwrap_or(DEFAULT_AUTO_COMMIT_TICK),
    }
}

/// Reverse-DNS identifier for this plugin.
pub const PLUGIN_ID: &str = "com.nexus.git";

/// IPC handler: returns current `GitState` as JSON.
pub const HANDLER_STATUS: u32 = 1;
/// IPC handler: returns recent commit log entries as JSON.
pub const HANDLER_LOG: u32 = 2;
/// IPC handler: returns local branches as JSON.
pub const HANDLER_BRANCHES: u32 = 3;
/// IPC handler: returns status of a single file (args: `{"path": "..."}`).
pub const HANDLER_FILE_STATUS: u32 = 4;
/// IPC handler: returns diff hunks for a file (args: `{"path": "..."}`).
pub const HANDLER_DIFF_FILE: u32 = 5;
/// IPC handler: stages a single file (args: `{"path": "..."}`).
pub const HANDLER_STAGE_FILE: u32 = 6;
/// IPC handler: unstages a single file (args: `{"path": "..."}`).
pub const HANDLER_UNSTAGE_FILE: u32 = 7;
/// IPC handler: creates a commit (args: `{"message": "..."}`).
pub const HANDLER_COMMIT: u32 = 8;
/// IPC handler: stages all modified files.
pub const HANDLER_STAGE_ALL: u32 = 9;
/// IPC handler: unstages all staged files.
pub const HANDLER_UNSTAGE_ALL: u32 = 10;
/// IPC handler: returns all changed files with their status.
pub const HANDLER_FILE_STATUSES: u32 = 11;
/// IPC handler: returns the diff of staged changes (index vs HEAD).
pub const HANDLER_DIFF_STAGED: u32 = 12;
/// IPC handler: switches to a branch (args: `{"name": "..."}`).
pub const HANDLER_SWITCH_BRANCH: u32 = 13;
/// IPC handler: creates a branch from HEAD (args: `{"name": "..."}`).
pub const HANDLER_CREATE_BRANCH: u32 = 14;
/// IPC handler: deletes a branch (args: `{"name": "..."}`).
pub const HANDLER_DELETE_BRANCH: u32 = 15;
/// IPC handler: pushes to a remote (args: `{"remote": "...", "branch": "..."}`).
pub const HANDLER_PUSH: u32 = 16;
/// IPC handler: stages specific hunks (args: `{"path": "...", "hunk_indices": [0, 1]}`).
pub const HANDLER_STAGE_HUNKS: u32 = 17;
/// IPC handler: unstages specific hunks (args: `{"path": "...", "hunk_indices": [0]}`).
pub const HANDLER_UNSTAGE_HUNKS: u32 = 18;
/// IPC handler: saves working-tree state to the stash (args: `{"message": "…"}` optional).
pub const HANDLER_STASH_PUSH: u32 = 23;
/// IPC handler: lists all stash entries.
pub const HANDLER_STASH_LIST: u32 = 24;
/// IPC handler: applies the top stash entry and removes it (args: `{"index": 0}` optional).
pub const HANDLER_STASH_POP: u32 = 25;
/// IPC handler: discards a stash entry without applying (args: `{"index": 0}` optional).
pub const HANDLER_STASH_DROP: u32 = 26;
/// IPC handler: lists all local tags.
pub const HANDLER_LIST_TAGS: u32 = 19;
/// IPC handler: creates a tag at HEAD (args: `{"name": "...", "message": "..."}`).
pub const HANDLER_CREATE_TAG: u32 = 20;
/// IPC handler: deletes a local tag (args: `{"name": "..."}`).
pub const HANDLER_DELETE_TAG: u32 = 21;
/// IPC handler: pushes all tags to a remote (args: `{"remote": "..."}`).
pub const HANDLER_PUSH_TAGS: u32 = 22;
/// IPC handler: report Git-LFS state (BL-091). No args; returns
/// `{ tracked_patterns, pointer_files, available_files,
///    git_lfs_installed }`. Inspects `.gitattributes` for `filter=lfs`
/// rules and walks the working tree classifying matched files.
pub const HANDLER_LFS_STATUS: u32 = 27;
/// IPC handler: non-interactive rebase onto a target branch
/// (BL-088). Args: `{"onto": "<branch>"}`. Returns
/// `{commits_rebased, conflicts}`; non-empty `conflicts` means the
/// rebase paused mid-flight and the caller should resolve + commit
/// manually or invoke `abort_rebase`.
pub const HANDLER_REBASE: u32 = 28;
/// IPC handler: abort an in-progress rebase (BL-088). No args.
pub const HANDLER_ABORT_REBASE: u32 = 29;
/// IPC handler: cherry-pick a single commit onto HEAD (BL-088).
/// Args: `{"commit": "<hash>"}`. Returns `{commit_hash, conflicts}`;
/// non-empty `conflicts` means the working tree holds the
/// in-progress state and the caller resolves manually or invokes
/// `abort_cherry_pick`.
pub const HANDLER_CHERRY_PICK: u32 = 30;
/// IPC handler: abort an in-progress cherry-pick (BL-088). No args.
pub const HANDLER_ABORT_CHERRY_PICK: u32 = 31;
/// IPC handler: list paths with unresolved merge conflicts (BL-084).
/// No args. Returns `["path/a", "path/b"]`. Empty when the index is
/// clean. Drives the shell conflict resolution panel's file list.
pub const HANDLER_CONFLICT_FILES: u32 = 32;
/// IPC handler: abort an in-progress merge (BL-084). No args. Mirrors
/// the existing `GitEngine::abort_merge` — restores pre-merge HEAD
/// via `reset --hard` + `cleanup_state`. Used by the shell conflict
/// panel's "Abort merge" button.
pub const HANDLER_ABORT_MERGE: u32 = 33;
/// IPC handler: read the three index-side versions of a conflicted
/// file (BL-084). Args: `{"path": "..."}`. Returns
/// `{base: <bytes-or-null>, ours: <bytes-or-null>, theirs: <bytes-or-null>}`
/// where each side is the raw blob bytes at that conflict stage.
/// Drives the shell conflict panel's three-way diff view.
pub const HANDLER_CONFLICT_VERSIONS: u32 = 34;
/// IPC handler: merge a branch into HEAD (BL-084). Args: `{"branch": "..."}`.
/// Returns `{fast_forward, conflicts, commit_hash}` mirroring
/// `MergeResult`. Drives the shell git panel's "Merge into branch"
/// flow; conflicts surface through the same `conflict_files` /
/// `conflict_versions` / `abort_merge` triple as everywhere else.
pub const HANDLER_MERGE: u32 = 35;
/// BL-079 IPC handler: blame annotations for a file. Args:
/// [`crate::ipc::GitPathArgs`]. Returns `Vec<GitBlameEntry>` —
/// one row per contiguous range of lines attributed to the same
/// commit. Drives the editor's inline-blame toggle so users see
/// "who last touched this line" without leaving the buffer.
pub const HANDLER_BLAME: u32 = 36;
/// BL-079 follow-up IPC handler: discard the selected working-tree
/// hunks of a file, restoring those line ranges to HEAD. Args:
/// [`crate::ipc::GitHunkArgs`] (same shape as `stage_hunks` /
/// `unstage_hunks`). The hunk indices match what
/// `com.nexus.git::diff_file` returned. Drives the editor gutter's
/// click-to-Revert affordance.
pub const HANDLER_DISCARD_HUNKS: u32 = 37;
/// P4-07 IPC handler: walk the commit log for a single file. Args
/// `{ path: String, limit?: u64 }`. Returns the same `LogEntry` shape
/// as [`HANDLER_LOG`] but filtered to commits that changed `path`.
/// Drives the editor tab-action "Open version history" — the shell
/// modal lists the entries and routes click-throughs to the
/// existing `diff_file`/`blame` surfaces.
pub const HANDLER_FILE_LOG: u32 = 38;

/// `worktree_list` (Phase 5.3 / RFC 0006) — list worktrees attached to the
/// forge repository. No args; returns [`crate::ipc::GitWorktreeListReply`].
pub const HANDLER_WORKTREE_LIST: u32 = 39;
/// `worktree_create` — add a worktree at `<forge>/.forge/worktrees/<name>`.
/// Args: [`crate::ipc::GitWorktreeCreateArgs`]; returns
/// [`crate::ipc::GitWorktreeInfo`].
pub const HANDLER_WORKTREE_CREATE: u32 = 40;
/// `worktree_remove` — remove a worktree. Args:
/// [`crate::ipc::GitWorktreeRemoveArgs`].
pub const HANDLER_WORKTREE_REMOVE: u32 = 41;

/// Plugin ids this plugin requires already loaded — `nexus-security`
/// provides the capability + credential types this crate uses.
pub const MANIFEST_DEPS: &[&str] = &["com.nexus.security"];

/// SD-06 — single source of truth for `(command-name, handler-id)`
/// pairs consumed by `nexus_bootstrap::plugins::git::register`. Order
/// matches the pre-SD-06 bootstrap registration so the emitted
/// manifest is byte-identical.
pub const IPC_HANDLERS: &[(&str, u32)] = &[
    ("status", HANDLER_STATUS),
    ("log", HANDLER_LOG),
    ("branches", HANDLER_BRANCHES),
    ("file_status", HANDLER_FILE_STATUS),
    ("diff_file", HANDLER_DIFF_FILE),
    ("stage_file", HANDLER_STAGE_FILE),
    ("unstage_file", HANDLER_UNSTAGE_FILE),
    ("commit", HANDLER_COMMIT),
    ("stage_all", HANDLER_STAGE_ALL),
    ("unstage_all", HANDLER_UNSTAGE_ALL),
    ("file_statuses", HANDLER_FILE_STATUSES),
    ("diff_staged", HANDLER_DIFF_STAGED),
    ("switch_branch", HANDLER_SWITCH_BRANCH),
    ("create_branch", HANDLER_CREATE_BRANCH),
    ("delete_branch", HANDLER_DELETE_BRANCH),
    ("push", HANDLER_PUSH),
    ("stage_hunks", HANDLER_STAGE_HUNKS),
    ("unstage_hunks", HANDLER_UNSTAGE_HUNKS),
    ("stash_push", HANDLER_STASH_PUSH),
    ("stash_list", HANDLER_STASH_LIST),
    ("stash_pop", HANDLER_STASH_POP),
    ("stash_drop", HANDLER_STASH_DROP),
    ("list_tags", HANDLER_LIST_TAGS),
    ("create_tag", HANDLER_CREATE_TAG),
    ("delete_tag", HANDLER_DELETE_TAG),
    ("push_tags", HANDLER_PUSH_TAGS),
    ("lfs_status", HANDLER_LFS_STATUS),
    ("rebase", HANDLER_REBASE),
    ("abort_rebase", HANDLER_ABORT_REBASE),
    ("cherry_pick", HANDLER_CHERRY_PICK),
    ("abort_cherry_pick", HANDLER_ABORT_CHERRY_PICK),
    ("conflict_files", HANDLER_CONFLICT_FILES),
    ("abort_merge", HANDLER_ABORT_MERGE),
    ("conflict_versions", HANDLER_CONFLICT_VERSIONS),
    ("merge", HANDLER_MERGE),
    ("blame", HANDLER_BLAME),
    ("discard_hunks", HANDLER_DISCARD_HUNKS),
    ("file_log", HANDLER_FILE_LOG),
    ("worktree_list", HANDLER_WORKTREE_LIST),
    ("worktree_create", HANDLER_WORKTREE_CREATE),
    ("worktree_remove", HANDLER_WORKTREE_REMOVE),
];

/// P2-06 — interval the background git-state watcher sleeps between
/// `git status` polls. Override per-forge via
/// `[git] poll_interval_secs = N` in `app.toml`.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);
const POLL_TICK: Duration = Duration::from_millis(200);

/// Core plugin that exposes git operations over IPC and publishes state-change
/// events to the kernel event bus.
pub struct GitCorePlugin {
    forge_root: PathBuf,
    event_bus: Option<Arc<EventBus>>,
    worker: Option<GitWorker>,
    poller_stop: Option<Arc<AtomicBool>>,
    poller_thread: Option<JoinHandle<()>>,
    auto_commit_stop: Option<Arc<AtomicBool>>,
    auto_commit_thread: Option<JoinHandle<()>>,
}

impl GitCorePlugin {
    /// Create an unstarted git core plugin for the given forge root.
    #[must_use]
    pub fn new(forge_root: PathBuf, event_bus: Option<Arc<EventBus>>) -> Self {
        Self {
            forge_root,
            event_bus,
            worker: None,
            poller_stop: None,
            poller_thread: None,
            auto_commit_stop: None,
            auto_commit_thread: None,
        }
    }
}

impl CorePlugin for GitCorePlugin {
    fn on_init(&mut self) -> Result<(), PluginError> {
        match GitWorker::spawn(&self.forge_root) {
            Ok(w) => {
                self.worker = Some(w);
                tracing::debug!(plugin_id = PLUGIN_ID, "git worker spawned");
            }
            Err(GitError::NotARepo(_)) => {
                tracing::info!(
                    plugin_id = PLUGIN_ID,
                    path = %self.forge_root.display(),
                    "forge root is not a git repository — git plugin is passive"
                );
            }
            Err(e) => {
                tracing::warn!(plugin_id = PLUGIN_ID, error = %e, "could not open git repo");
            }
        }
        Ok(())
    }

    fn on_start(&mut self) -> Result<(), PluginError> {
        let Some(w) = &self.worker else {
            return Ok(());
        };
        let handle = w.handle();
        let bus = self.event_bus.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);

        let timing = read_git_settings(&self.forge_root);
        let poll_interval = timing.poll_interval;
        let thread = thread::Builder::new()
            .name("nexus-git-poller".to_string())
            .spawn(move || run_poller(handle, bus, stop_clone, poll_interval))
            .map_err(|e| PluginError::LifecycleError {
                plugin_id: PLUGIN_ID.to_string(),
                hook: "on_start".to_string(),
                reason: format!("failed to spawn git poller thread: {e}"),
            })?;

        self.poller_stop = Some(stop);
        self.poller_thread = Some(thread);
        tracing::info!(plugin_id = PLUGIN_ID, "git state poller started");

        // Spawn background auto-commit thread if enabled in forge config.
        if timing.auto_commit {
            let ac_interval = timing.auto_commit_interval_secs;
            let ac_tick = timing.auto_commit_tick;
            let ac_root = self.forge_root.clone();
            let ac_bus = self.event_bus.clone();
            let ac_stop = Arc::new(AtomicBool::new(false));
            let ac_clone = Arc::clone(&ac_stop);
            let ac_thread = thread::Builder::new()
                .name("nexus-git-auto-commit".to_string())
                .spawn(move || run_auto_committer(ac_root, ac_interval, ac_tick, ac_bus, ac_clone))
                .map_err(|e| PluginError::LifecycleError {
                    plugin_id: PLUGIN_ID.to_string(),
                    hook: "on_start".to_string(),
                    reason: format!("failed to spawn auto-commit thread: {e}"),
                })?;
            self.auto_commit_stop = Some(ac_stop);
            self.auto_commit_thread = Some(ac_thread);
            tracing::info!(
                plugin_id = PLUGIN_ID,
                interval_secs = ac_interval,
                "auto-commit thread started"
            );
        }

        Ok(())
    }

    fn on_stop(&mut self) {
        if let Some(stop) = self.poller_stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
        if let Some(t) = self.poller_thread.take() {
            let _ = t.join();
        }
        tracing::info!(plugin_id = PLUGIN_ID, "git state poller stopped");

        if let Some(stop) = self.auto_commit_stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
        if let Some(t) = self.auto_commit_thread.take() {
            let _ = t.join();
        }
    }

    #[allow(clippy::too_many_lines)]
    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        use crate::handlers::{branches, log, merge, staging, stash, status, tags, worktree};

        let Some(w) = &self.worker else {
            // Passive mode — forge root is not a git repository. HANDLER_STATUS
            // returns JSON null so the shell-side gitStatus plugin can quietly
            // set status=null without hitting the PluginCrashedDuringCall path.
            // All other handlers are not meaningful without a repo and return
            // an explicit error so callers know the call was rejected.
            if handler_id == HANDLER_STATUS {
                return Ok(serde_json::Value::Null);
            }
            return Err(PluginError::ExecutionFailed {
                plugin_id: PLUGIN_ID.to_string(),
                reason: "forge root is not a git repository".to_string(),
            });
        };
        let h = w.handle();
        let root = self.forge_root.as_path();

        match handler_id {
            HANDLER_STATUS => status::status(&h),
            HANDLER_LOG => log::log(&h, args),
            HANDLER_BRANCHES => branches::branches(&h),
            HANDLER_FILE_STATUS => status::file_status(&h, args, root),
            HANDLER_DIFF_FILE => log::diff_file(&h, args, root),
            HANDLER_STAGE_FILE => staging::stage_file(&h, args, root),
            HANDLER_UNSTAGE_FILE => staging::unstage_file(&h, args, root),
            HANDLER_COMMIT => staging::commit(&h, args),
            HANDLER_STAGE_ALL => staging::stage_all(&h),
            HANDLER_UNSTAGE_ALL => staging::unstage_all(&h),
            HANDLER_FILE_STATUSES => status::file_statuses(&h),
            HANDLER_DIFF_STAGED => log::diff_staged(&h),
            HANDLER_SWITCH_BRANCH => branches::switch_branch(&h, args),
            HANDLER_CREATE_BRANCH => branches::create_branch(&h, args),
            HANDLER_DELETE_BRANCH => branches::delete_branch(&h, args),
            HANDLER_PUSH => branches::push(&h, args),
            HANDLER_STAGE_HUNKS => staging::stage_hunks(&h, args, root),
            HANDLER_UNSTAGE_HUNKS => staging::unstage_hunks(&h, args, root),
            HANDLER_DISCARD_HUNKS => staging::discard_hunks(&h, args, root),
            HANDLER_FILE_LOG => log::file_log(&h, args, root),
            HANDLER_STASH_PUSH => stash::stash_push(&h, args),
            HANDLER_STASH_LIST => stash::stash_list(&h),
            HANDLER_STASH_POP => stash::stash_pop(&h, args),
            HANDLER_STASH_DROP => stash::stash_drop(&h, args),
            HANDLER_LIST_TAGS => tags::list_tags(&h),
            HANDLER_CREATE_TAG => tags::create_tag(&h, args),
            HANDLER_DELETE_TAG => tags::delete_tag(&h, args),
            HANDLER_PUSH_TAGS => tags::push_tags(&h, args),
            HANDLER_LFS_STATUS => Ok(status::lfs_status(root)),
            HANDLER_REBASE => merge::rebase(&h, args),
            HANDLER_ABORT_REBASE => merge::abort_rebase(&h),
            HANDLER_CHERRY_PICK => merge::cherry_pick(&h, args),
            HANDLER_ABORT_CHERRY_PICK => merge::abort_cherry_pick(&h),
            HANDLER_CONFLICT_FILES => merge::conflict_files(&h),
            HANDLER_ABORT_MERGE => merge::abort_merge(&h),
            HANDLER_CONFLICT_VERSIONS => merge::conflict_versions(&h, args),
            HANDLER_MERGE => merge::merge(&h, args),
            HANDLER_BLAME => status::blame(&h, args, root),
            HANDLER_WORKTREE_LIST => worktree::worktree_list(&h),
            HANDLER_WORKTREE_CREATE => worktree::worktree_create(&h, args, root),
            HANDLER_WORKTREE_REMOVE => worktree::worktree_remove(&h, args),
            _ => Err(PluginError::ExecutionFailed {
                plugin_id: PLUGIN_ID.to_string(),
                reason: format!("unknown handler_id {handler_id}"),
            }),
        }
    }
}

/// BL-091 — snapshot of Git-LFS state for `lfs_status`.
///
/// Public re-export of the LFS snapshot logic now owned by
/// [`crate::handlers::status::lfs_status`]. Kept here as the
/// stable nexus-cli entry point — the SD-03 split moved the
/// implementation but the surface stays at `core_plugin`.
#[doc(hidden)]
pub fn lfs_status_for_forge(forge_root: &Path) -> serde_json::Value {
    crate::handlers::status::lfs_status(forge_root)
}

// `run_poller` is spawned as a thread and takes ownership of handle,
// bus, and stop for the thread's lifetime — the lint fires because the
// body only needs &handle / &bus / &stop, but hoisting the moves into
// the caller would duplicate the thread::spawn boilerplate.
#[allow(clippy::needless_pass_by_value)]
fn run_poller(
    handle: GitWorkerHandle,
    bus: Option<Arc<EventBus>>,
    stop: Arc<AtomicBool>,
    poll_interval: Duration,
) {
    let mut prev: Option<GitState> = None;

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }

        match handle.with(|e| e.state()) {
            Ok(state) => {
                if let Some(ref bus) = bus {
                    publish_changes(bus, prev.as_ref(), &state);
                }
                prev = Some(state);
            }
            Err(e) => {
                tracing::debug!(plugin_id = PLUGIN_ID, error = %e, "git poll error");
            }
        }

        let mut waited = Duration::ZERO;
        while waited < poll_interval {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            thread::sleep(POLL_TICK);
            waited += POLL_TICK;
        }
    }
}

fn publish_changes(bus: &EventBus, prev: Option<&GitState>, curr: &GitState) {
    let Some(prev) = prev else {
        let _ = bus.publish_plugin(
            PLUGIN_ID,
            "com.nexus.git.state",
            json!({
                "branch": curr.branch,
                "head": curr.head_oid,
                "is_dirty": curr.is_dirty,
                "repo_state": format!("{:?}", curr.repo_state),
                "tracking": curr.tracking_oid,
                "upstream": curr.upstream,
            }),
        );
        return;
    };

    if prev.branch != curr.branch {
        let _ = bus.publish_plugin(
            PLUGIN_ID,
            "com.nexus.git.branch_changed",
            json!({
                "from": prev.branch,
                "to": curr.branch,
                "head": curr.head_oid,
            }),
        );
    }

    if prev.head_oid != curr.head_oid {
        let _ = bus.publish_plugin(
            PLUGIN_ID,
            "com.nexus.git.commit",
            json!({
                "branch": curr.branch,
                "head": curr.head_oid,
                "prev_head": prev.head_oid,
            }),
        );
        // BL-052 — detected HEAD change reaches the universal activity
        // timeline as a commit-class entry. Branch / dirty events stay
        // out — branch-only churn isn't audit-worthy.
        publish_git_activity(bus, "commit", &curr.head_oid, curr.branch.as_deref());
    }

    if prev.is_dirty != curr.is_dirty {
        let _ = bus.publish_plugin(
            PLUGIN_ID,
            "com.nexus.git.dirty_changed",
            json!({
                "is_dirty": curr.is_dirty,
                "branch": curr.branch,
                "head": curr.head_oid,
            }),
        );
    }

    // BL-052 follow-up — detect remote-side push / fetch via the
    // upstream tracking branch's SHA. A change here without a local
    // HEAD change means either the user fetched (new commits on the
    // remote arrived locally as `refs/remotes/<remote>/<branch>`)
    // or pushed (modern git updates the local tracking ref to
    // reflect what was just sent). Either way the activity timeline
    // wants to know.
    //
    // Skip the first observation (`prev.tracking_oid` is `None`):
    // detecting a "change" against a missing prior is just saying
    // "the upstream existed all along", which isn't a meaningful
    // event.
    if prev.tracking_oid != curr.tracking_oid && prev.tracking_oid.is_some() {
        let _ = bus.publish_plugin(
            PLUGIN_ID,
            "com.nexus.git.remote_changed",
            json!({
                "branch": curr.branch,
                "upstream": curr.upstream,
                "head": curr.head_oid,
                "tracking": curr.tracking_oid,
                "prev_tracking": prev.tracking_oid,
            }),
        );
        let head_for_activity = curr
            .tracking_oid
            .as_deref()
            .unwrap_or(curr.head_oid.as_str());
        publish_git_activity(
            bus,
            "remote_changed",
            head_for_activity,
            curr.upstream.as_deref().or(curr.branch.as_deref()),
        );
    }
}

/// BL-052 — publish a git event onto the universal activity topic.
/// `kind` is a short verb (`commit`, `branch_changed`, etc.); `head`
/// is the relevant short hash; `branch` carries the optional branch
/// name. Best-effort — bus failures are logged at debug and swallowed.
fn publish_git_activity(bus: &EventBus, kind: &str, head: &str, branch: Option<&str>) {
    use nexus_types::activity::{
        ActivityEntry, ActivityOrigin, ActivityOutcome, ActivitySurface, ACTIVITY_APPENDED_TOPIC,
    };
    let mut entry = ActivityEntry::now(head.to_string(), ActivitySurface::Git, ActivityOrigin::Git);
    entry.outcome = ActivityOutcome::Ok;
    let head_short: String = head.chars().take(7).collect();
    entry.prompt = match branch {
        Some(b) => format!("{kind} {head_short} on {b}"),
        None => format!("{kind} {head_short}"),
    };
    if let Ok(payload) = serde_json::to_value(&entry) {
        if let Err(err) = bus.publish_plugin(PLUGIN_ID, ACTIVITY_APPENDED_TOPIC, payload) {
            tracing::debug!(plugin_id = PLUGIN_ID, %err, "failed to publish git activity");
        }
    }
}

// ── Auto-commit background loop ───────────────────────────────────────────────
//
// Wakes on a 30-second tick. Drains `com.nexus.storage.file_modified` events
// from the bus to track when the forge was last edited. After `idle_secs` of
// no file modifications, stages everything and commits.

/// P2-06 — interval the auto-commit background loop wakes on to
/// re-check the idle window. Distinct from the user-facing
/// `[git] auto_commit_interval_secs` (= the idle window itself);
/// this is the polling cadence within that window. Override per-forge
/// via `[git] auto_commit_tick_secs = N` in `app.toml`.
pub const DEFAULT_AUTO_COMMIT_TICK: Duration = Duration::from_secs(30);

#[allow(clippy::needless_pass_by_value)]
fn run_auto_committer(
    forge_root: PathBuf,
    idle_secs: u64,
    tick: Duration,
    bus: Option<Arc<EventBus>>,
    stop: Arc<AtomicBool>,
) {
    let mut committer = AutoCommitter::new(&forge_root, 0); // debounce handled externally
    let idle = Duration::from_secs(idle_secs);
    let mut last_modified: Option<Instant> = None;

    let mut sub = bus.as_ref().map(|b| {
        b.subscribe(EventFilter::CustomPrefix(
            "com.nexus.storage.file_modified".to_string(),
        ))
    });

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }

        // Drain file-modified events — each one refreshes the idle timer.
        if let Some(ref mut s) = sub {
            while let Ok(Some(_)) = s.try_recv() {
                last_modified = Some(Instant::now());
            }
        }

        // Commit once we've been idle for the configured window.
        if let Some(t) = last_modified {
            if t.elapsed() >= idle {
                committer.reset_debounce();
                match committer.check_and_commit() {
                    Ok(r) if r.commit_hash.is_some() => {
                        tracing::info!(
                            plugin_id = PLUGIN_ID,
                            hash = r.commit_hash.as_deref().unwrap_or(""),
                            files = r.files_changed,
                            "auto-commit: {}",
                            r.message.as_deref().unwrap_or(""),
                        );
                        if let Some(ref b) = bus {
                            // BL-052 — well-formed `ActivityEntry`
                            // payload with `origin: "git"` and surface
                            // `git`. Carries the commit hash + message
                            // in `prompt`; the affected file count
                            // surfaces via the `files_changed` field
                            // mirrored into `tool_calls` (one synthetic
                            // entry) so the UI can display "N files".
                            use nexus_types::activity::{
                                ActivityEntry, ActivityOrigin, ActivityOutcome, ActivitySurface,
                                ActivityToolCall, ACTIVITY_APPENDED_TOPIC,
                            };
                            let mut entry = ActivityEntry::now(
                                r.commit_hash.clone().unwrap_or_default(),
                                ActivitySurface::Git,
                                ActivityOrigin::Git,
                            );
                            entry.outcome = ActivityOutcome::Ok;
                            let hash_short = r
                                .commit_hash
                                .as_deref()
                                .map_or(String::new(), |h| h.chars().take(7).collect());
                            entry.prompt = format!(
                                "auto-commit {} {}",
                                hash_short,
                                r.message.as_deref().unwrap_or(""),
                            );
                            entry.tool_calls.push(ActivityToolCall {
                                name: format!("{} files changed", r.files_changed),
                                ok: true,
                            });
                            if let Ok(payload) = serde_json::to_value(&entry) {
                                let _ =
                                    b.publish_plugin(PLUGIN_ID, ACTIVITY_APPENDED_TOPIC, payload);
                            }
                        }
                        last_modified = None;
                    }
                    Err(e) => {
                        tracing::debug!(plugin_id = PLUGIN_ID, error = %e, "auto-commit failed");
                    }
                    _ => {}
                }
            }
        }

        // Sleep in small ticks so the stop flag is checked promptly.
        let mut waited = Duration::ZERO;
        while waited < tick {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            thread::sleep(POLL_TICK);
            waited += POLL_TICK;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::tempdir;

    fn init_repo(path: &std::path::Path) {
        let ok = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(path)
            .status()
            .expect("git init");
        assert!(ok.success());
        // Disable commit signing locally — some dev/CI environments
        // configure `commit.gpgsign=true` globally, which would make
        // the `git commit` calls below fail silently (the tests use
        // `let _ = …` to ignore exit status). Without a commit the
        // branch ref is never created, and downstream assertions on
        // `state().branch` see `None` instead of `Some("main")`.
        let _ = Command::new("git")
            .args(["config", "commit.gpgsign", "false"])
            .current_dir(path)
            .status();
    }

    #[test]
    fn plugin_id_constant_is_correct() {
        assert_eq!(PLUGIN_ID, "com.nexus.git");
    }

    #[test]
    fn on_init_succeeds_in_non_repo_directory() {
        let dir = tempdir().unwrap();
        let mut plugin = GitCorePlugin::new(dir.path().to_path_buf(), None);
        assert!(plugin.on_init().is_ok());
        assert!(plugin.worker.is_none());
    }

    #[test]
    fn on_init_spawns_worker_in_git_repo() {
        let dir = tempdir().unwrap();
        init_repo(dir.path());
        let mut plugin = GitCorePlugin::new(dir.path().to_path_buf(), None);
        assert!(plugin.on_init().is_ok());
        assert!(plugin.worker.is_some());
    }

    #[test]
    fn dispatch_status_returns_git_state() {
        let dir = tempdir().unwrap();
        init_repo(dir.path());
        let mut plugin = GitCorePlugin::new(dir.path().to_path_buf(), None);
        plugin.on_init().unwrap();
        let result = plugin.dispatch(HANDLER_STATUS, &json!({}));
        assert!(result.is_ok(), "status dispatch failed: {result:?}");
        let v = result.unwrap();
        assert!(v.get("head").is_some());
        assert!(v.get("is_dirty").is_some());
    }

    #[test]
    fn dispatch_unknown_handler_returns_error() {
        let dir = tempdir().unwrap();
        init_repo(dir.path());
        let mut plugin = GitCorePlugin::new(dir.path().to_path_buf(), None);
        plugin.on_init().unwrap();
        let result = plugin.dispatch(999, &json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn dispatch_status_without_repo_returns_null() {
        let dir = tempdir().unwrap();
        let mut plugin = GitCorePlugin::new(dir.path().to_path_buf(), None);
        plugin.on_init().unwrap();
        // Passive mode: HANDLER_STATUS returns JSON null so the shell can
        // set status=null without hitting the PluginCrashedDuringCall path.
        let result = plugin.dispatch(HANDLER_STATUS, &json!({}));
        assert!(result.is_ok(), "expected Ok, got {result:?}");
        assert_eq!(result.unwrap(), serde_json::Value::Null);
    }

    #[test]
    fn dispatch_non_status_without_repo_returns_error() {
        let dir = tempdir().unwrap();
        let mut plugin = GitCorePlugin::new(dir.path().to_path_buf(), None);
        plugin.on_init().unwrap();
        // Non-status handlers are rejected in passive mode.
        let result = plugin.dispatch(HANDLER_LOG, &json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn on_start_and_stop_in_git_repo() {
        let dir = tempdir().unwrap();
        init_repo(dir.path());
        let mut plugin = GitCorePlugin::new(dir.path().to_path_buf(), None);
        plugin.on_init().unwrap();
        plugin.on_start().unwrap();
        assert!(plugin.poller_thread.is_some());
        plugin.on_stop();
        assert!(plugin.poller_thread.is_none());
    }

    #[test]
    fn publish_changes_emits_initial_state_event() {
        let bus = Arc::new(EventBus::new(16));
        let mut sub = bus.subscribe(nexus_kernel::EventFilter::CustomPrefix(
            "com.nexus.git.".to_string(),
        ));
        let state = GitState {
            branch: Some("main".to_string()),
            head_oid: "abc1234".to_string(),
            is_dirty: false,
            repo_state: crate::RepoState::Clean,
            tracking_oid: None,
            upstream: None,
        };
        publish_changes(&bus, None, &state);
        let ev = sub.try_recv().unwrap().unwrap();
        match &ev.event {
            nexus_kernel::NexusEvent::Custom { type_id, .. } => {
                assert_eq!(type_id, "com.nexus.git.state");
            }
            _ => panic!("expected Custom event"),
        }
    }

    #[test]
    fn publish_changes_emits_branch_changed_event() {
        let bus = Arc::new(EventBus::new(16));
        let mut sub = bus.subscribe(nexus_kernel::EventFilter::CustomPrefix(
            "com.nexus.git.".to_string(),
        ));
        let prev = GitState {
            branch: Some("main".to_string()),
            head_oid: "abc1234".to_string(),
            is_dirty: false,
            repo_state: crate::RepoState::Clean,
            tracking_oid: None,
            upstream: None,
        };
        let curr = GitState {
            branch: Some("feature".to_string()),
            head_oid: "abc1234".to_string(),
            is_dirty: false,
            repo_state: crate::RepoState::Clean,
            tracking_oid: None,
            upstream: None,
        };
        publish_changes(&bus, Some(&prev), &curr);
        let ev = sub.try_recv().unwrap().unwrap();
        match &ev.event {
            nexus_kernel::NexusEvent::Custom {
                type_id, payload, ..
            } => {
                assert_eq!(type_id, "com.nexus.git.branch_changed");
                assert_eq!(payload["to"], "feature");
            }
            _ => panic!("expected Custom event"),
        }
    }

    /// BL-052 follow-up — when the upstream tracking-branch SHA
    /// changes between polls (a fetch or push happened externally),
    /// the poller emits `com.nexus.git.remote_changed` and an
    /// activity entry. Both prev and curr have the same head_oid so
    /// this test isolates the new branch.
    #[test]
    fn publish_changes_emits_remote_changed_on_tracking_oid_advance() {
        use nexus_types::activity::ACTIVITY_APPENDED_TOPIC;
        let bus = Arc::new(EventBus::new(16));
        let mut sub_git = bus.subscribe(nexus_kernel::EventFilter::CustomPrefix(
            "com.nexus.git.".to_string(),
        ));
        let mut sub_activity = bus.subscribe(nexus_kernel::EventFilter::CustomPrefix(
            ACTIVITY_APPENDED_TOPIC.to_string(),
        ));
        let prev = GitState {
            branch: Some("main".to_string()),
            head_oid: "abc1234".to_string(),
            is_dirty: false,
            repo_state: crate::RepoState::Clean,
            tracking_oid: Some("aaaaaaa".to_string()),
            upstream: Some("origin/main".to_string()),
        };
        let curr = GitState {
            branch: Some("main".to_string()),
            head_oid: "abc1234".to_string(),
            is_dirty: false,
            repo_state: crate::RepoState::Clean,
            tracking_oid: Some("bbbbbbb".to_string()),
            upstream: Some("origin/main".to_string()),
        };
        publish_changes(&bus, Some(&prev), &curr);

        // remote_changed event with prev + curr tracking SHAs.
        let mut saw_remote_changed = false;
        while let Ok(Some(ev)) = sub_git.try_recv() {
            if let nexus_kernel::NexusEvent::Custom {
                type_id, payload, ..
            } = &ev.event
            {
                if type_id == "com.nexus.git.remote_changed" {
                    saw_remote_changed = true;
                    assert_eq!(payload["upstream"], "origin/main");
                    assert_eq!(payload["tracking"], "bbbbbbb");
                    assert_eq!(payload["prev_tracking"], "aaaaaaa");
                }
            }
        }
        assert!(saw_remote_changed, "expected com.nexus.git.remote_changed");

        // Activity entry with kind "remote_changed".
        let mut saw_activity = false;
        while let Ok(Some(ev)) = sub_activity.try_recv() {
            if let nexus_kernel::NexusEvent::Custom {
                type_id, payload, ..
            } = &ev.event
            {
                if type_id == ACTIVITY_APPENDED_TOPIC {
                    let prompt = payload["prompt"].as_str().unwrap_or("");
                    assert!(prompt.starts_with("remote_changed"), "got: {prompt}");
                    assert!(prompt.contains("origin/main"), "got: {prompt}");
                    saw_activity = true;
                }
            }
        }
        assert!(saw_activity, "expected universal-activity entry");
    }

    /// First observation never emits remote_changed — without a prior
    /// tracking_oid the change is "the upstream existed all along",
    /// which isn't a meaningful event.
    #[test]
    fn publish_changes_skips_remote_changed_on_first_observation() {
        let bus = Arc::new(EventBus::new(16));
        let mut sub = bus.subscribe(nexus_kernel::EventFilter::CustomPrefix(
            "com.nexus.git.".to_string(),
        ));
        let prev = GitState {
            branch: Some("main".to_string()),
            head_oid: "abc1234".to_string(),
            is_dirty: false,
            repo_state: crate::RepoState::Clean,
            tracking_oid: None,
            upstream: None,
        };
        let curr = GitState {
            branch: Some("main".to_string()),
            head_oid: "abc1234".to_string(),
            is_dirty: false,
            repo_state: crate::RepoState::Clean,
            tracking_oid: Some("bbbbbbb".to_string()),
            upstream: Some("origin/main".to_string()),
        };
        publish_changes(&bus, Some(&prev), &curr);
        while let Ok(Some(ev)) = sub.try_recv() {
            if let nexus_kernel::NexusEvent::Custom { type_id, .. } = &ev.event {
                assert_ne!(type_id, "com.nexus.git.remote_changed");
            }
        }
    }

    /// State inspection: the engine populates `tracking_oid` +
    /// `upstream` for a branch with a configured upstream. Backed by
    /// a tempdir repo with a fake remote-tracking ref so the assertion
    /// doesn't require network access.
    #[test]
    fn state_populates_tracking_oid_when_upstream_is_configured() {
        let dir = tempdir().unwrap();
        init_repo(dir.path());
        let _ = Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(dir.path())
            .status();
        let _ = Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .status();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let _ = Command::new("git")
            .args(["add", "a.txt"])
            .current_dir(dir.path())
            .status();
        let _ = Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir.path())
            .status();
        // Rename the active branch to "main" if it isn't already.
        let _ = Command::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(dir.path())
            .status();
        // Register a fake `origin` remote — libgit2's
        // `Branch::upstream()` walks the config to resolve the
        // tracking branch, and rejects branches that name a remote
        // not in `remote.<name>.url`. URL doesn't have to be
        // reachable; nothing here actually fetches.
        let _ = Command::new("git")
            .args(["remote", "add", "origin", "file:///tmp/nexus-test-origin"])
            .current_dir(dir.path())
            .status();
        // Fake a remote-tracking ref pointing at the same commit.
        // `git update-ref refs/remotes/origin/main HEAD` is the
        // minimal way to seed an upstream without a real fetch.
        let _ = Command::new("git")
            .args(["update-ref", "refs/remotes/origin/main", "HEAD"])
            .current_dir(dir.path())
            .status();
        // Wire `branch.main.remote` + `branch.main.merge` so libgit2
        // recognizes the upstream relationship.
        let _ = Command::new("git")
            .args(["config", "branch.main.remote", "origin"])
            .current_dir(dir.path())
            .status();
        let _ = Command::new("git")
            .args(["config", "branch.main.merge", "refs/heads/main"])
            .current_dir(dir.path())
            .status();

        let engine = crate::GitEngine::open(dir.path()).expect("open");
        let st = engine.state().expect("state");
        assert_eq!(st.branch.as_deref(), Some("main"));
        assert!(
            st.tracking_oid.is_some(),
            "tracking_oid should populate when upstream is configured",
        );
        assert_eq!(st.upstream.as_deref(), Some("origin/main"));
    }

    /// BL-079 — `blame` handler returns one entry per committed
    /// line range with the right shape. The repo is a fresh init
    /// with one commit; every line in the file should attribute to
    /// that single commit.
    #[test]
    fn blame_handler_returns_entries_for_committed_file() {
        let dir = tempdir().unwrap();
        init_repo(dir.path());
        // Configure committer identity — `git commit` rejects
        // operations without it on systems where it isn't already
        // set globally (e.g. CI containers).
        let _ = Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(dir.path())
            .status();
        let _ = Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .status();

        std::fs::write(dir.path().join("hello.txt"), "alpha\nbeta\n").unwrap();
        let _ = Command::new("git")
            .args(["add", "hello.txt"])
            .current_dir(dir.path())
            .status();
        let _ = Command::new("git")
            .args(["commit", "-m", "init", "--quiet"])
            .current_dir(dir.path())
            .status();

        let mut plugin = GitCorePlugin::new(dir.path().to_path_buf(), None);
        plugin.on_init().unwrap();
        let resp = plugin
            .dispatch(HANDLER_BLAME, &json!({ "path": "hello.txt" }))
            .expect("blame ok");
        let arr = resp.as_array().expect("array");
        assert!(!arr.is_empty(), "expected at least one blame entry");
        let first = &arr[0];
        // Every entry mirrors GitBlameEntry's shape.
        assert!(first["commit_hash"].is_string());
        assert!(first["author"].is_string());
        assert!(first["date"].is_string());
        assert!(first["message"].is_string());
        assert!(first["start_line"].is_u64());
        assert!(first["end_line"].is_u64());
        assert!(
            first["start_line"].as_u64().unwrap() >= 1,
            "start_line is 1-based"
        );
    }
}
