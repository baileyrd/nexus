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
}

fn default_interval() -> u64 { 1800 }

impl Default for AutoCommitGitSettings {
    fn default() -> Self {
        Self { auto_commit: false, auto_commit_interval_secs: default_interval() }
    }
}

fn read_auto_commit_settings(forge_root: &Path) -> (bool, u64) {
    let path = forge_root.join(".forge").join("app.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return (false, default_interval()),
    };
    let cfg: AutoCommitAppConfig = toml::from_str(&text).unwrap_or_default();
    (cfg.git.auto_commit, cfg.git.auto_commit_interval_secs)
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

const POLL_INTERVAL: Duration = Duration::from_secs(2);
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

        let thread = thread::Builder::new()
            .name("nexus-git-poller".to_string())
            .spawn(move || run_poller(handle, bus, stop_clone))
            .map_err(|e| PluginError::LifecycleError {
                plugin_id: PLUGIN_ID.to_string(),
                hook: "on_start".to_string(),
                reason: format!("failed to spawn git poller thread: {e}"),
            })?;

        self.poller_stop = Some(stop);
        self.poller_thread = Some(thread);
        tracing::info!(plugin_id = PLUGIN_ID, "git state poller started");

        // Spawn background auto-commit thread if enabled in forge config.
        let (ac_enabled, ac_interval) = read_auto_commit_settings(&self.forge_root);
        if ac_enabled {
            let ac_root  = self.forge_root.clone();
            let ac_bus   = self.event_bus.clone();
            let ac_stop  = Arc::new(AtomicBool::new(false));
            let ac_clone = Arc::clone(&ac_stop);
            let ac_thread = thread::Builder::new()
                .name("nexus-git-auto-commit".to_string())
                .spawn(move || run_auto_committer(ac_root, ac_interval, ac_bus, ac_clone))
                .map_err(|e| PluginError::LifecycleError {
                    plugin_id: PLUGIN_ID.to_string(),
                    hook: "on_start".to_string(),
                    reason: format!("failed to spawn auto-commit thread: {e}"),
                })?;
            self.auto_commit_stop = Some(ac_stop);
            self.auto_commit_thread = Some(ac_thread);
            tracing::info!(plugin_id = PLUGIN_ID, interval_secs = ac_interval, "auto-commit thread started");
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

        match handler_id {
            HANDLER_STATUS => {
                let state = h.with(|e| e.state()).map_err(map_err)?;
                Ok(json!({
                    "branch": state.branch,
                    "head": state.head_oid,
                    "is_dirty": state.is_dirty,
                    "repo_state": format!("{:?}", state.repo_state),
                }))
            }
            HANDLER_LOG => {
                let limit = args
                    .get("limit")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|v| usize::try_from(v).ok())
                    .unwrap_or(20);
                let entries = h.with(move |e| e.log(limit)).map_err(map_err)?;
                let arr: Vec<_> = entries
                    .iter()
                    .map(|le| {
                        json!({
                            "hash": le.hash,
                            "author": le.author,
                            "date": le.date.to_rfc3339(),
                            "message": le.message,
                            "parents": le.parents,
                        })
                    })
                    .collect();
                Ok(serde_json::Value::Array(arr))
            }
            HANDLER_BRANCHES => {
                let branches = h.with(|e| e.branches()).map_err(map_err)?;
                let arr: Vec<_> = branches
                    .iter()
                    .map(|b| {
                        json!({
                            "name": b.name,
                            "is_head": b.is_head,
                            "upstream": b.upstream,
                        })
                    })
                    .collect();
                Ok(serde_json::Value::Array(arr))
            }
            HANDLER_FILE_STATUS => {
                let path = path_arg(args, &self.forge_root)?;
                let status = h.with(move |e| e.file_status(&path)).map_err(map_err)?;
                Ok(json!(status.marker()))
            }
            HANDLER_DIFF_FILE => {
                let path = path_arg(args, &self.forge_root)?;
                let hunks = h.with(move |e| e.diff_file(&path)).map_err(map_err)?;
                let arr: Vec<_> = hunks
                    .iter()
                    .map(|hunk| {
                        json!({
                            "old_start": hunk.old_start,
                            "old_count": hunk.old_count,
                            "new_start": hunk.new_start,
                            "new_count": hunk.new_count,
                            "lines": hunk.lines.iter().map(|l| json!({
                                "kind": format!("{:?}", l.kind),
                                "content": l.content,
                            })).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                Ok(serde_json::Value::Array(arr))
            }
            HANDLER_STAGE_FILE => {
                let path = path_arg(args, &self.forge_root)?;
                h.with(move |e| e.stage_file(&path)).map_err(map_err)?;
                Ok(json!({"ok": true}))
            }
            HANDLER_UNSTAGE_FILE => {
                let path = path_arg(args, &self.forge_root)?;
                h.with(move |e| e.unstage_file(&path)).map_err(map_err)?;
                Ok(json!({"ok": true}))
            }
            HANDLER_COMMIT => {
                let msg = args
                    .get("message")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: "missing 'message' argument".to_string(),
                    })?
                    .to_string();
                let hash = h.with(move |e| e.commit(&msg)).map_err(map_err)?;
                Ok(json!({"hash": hash}))
            }
            HANDLER_STAGE_ALL => {
                h.with(|e| e.stage_all()).map_err(map_err)?;
                Ok(json!({"ok": true}))
            }
            HANDLER_UNSTAGE_ALL => {
                h.with(|e| e.unstage_all()).map_err(map_err)?;
                Ok(json!({"ok": true}))
            }
            HANDLER_FILE_STATUSES => {
                let statuses = h.with(|e| e.file_statuses()).map_err(map_err)?;
                let arr: Vec<_> = statuses
                    .iter()
                    .map(|s| json!({
                        "path": s.path.to_string_lossy(),
                        "status": format!("{:?}", s.status),
                    }))
                    .collect();
                Ok(serde_json::Value::Array(arr))
            }
            HANDLER_DIFF_STAGED => {
                let diffs = h.with(|e| e.diff_staged()).map_err(map_err)?;
                let arr: Vec<_> = diffs
                    .iter()
                    .map(|(path, hunks)| {
                        json!({
                            "path": path,
                            "hunks": hunks.iter().map(|hunk| json!({
                                "old_start": hunk.old_start,
                                "old_count": hunk.old_count,
                                "new_start": hunk.new_start,
                                "new_count": hunk.new_count,
                                "lines": hunk.lines.iter().map(|l| json!({
                                    "kind": format!("{:?}", l.kind),
                                    "content": l.content,
                                })).collect::<Vec<_>>(),
                            })).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                Ok(serde_json::Value::Array(arr))
            }
            HANDLER_SWITCH_BRANCH => {
                let name = string_arg(args, "name")?;
                h.with(move |e| e.switch_branch(&name)).map_err(map_err)?;
                Ok(json!({"ok": true}))
            }
            HANDLER_CREATE_BRANCH => {
                let name = string_arg(args, "name")?;
                h.with(move |e| e.create_branch(&name)).map_err(map_err)?;
                Ok(json!({"ok": true}))
            }
            HANDLER_DELETE_BRANCH => {
                let name = string_arg(args, "name")?;
                h.with(move |e| e.delete_branch(&name)).map_err(map_err)?;
                Ok(json!({"ok": true}))
            }
            HANDLER_PUSH => {
                let remote = string_arg(args, "remote")?;
                let branch = string_arg(args, "branch")?;
                h.with(move |e| e.push(&remote, &branch)).map_err(map_err)?;
                Ok(json!({"ok": true}))
            }
            HANDLER_STAGE_HUNKS => {
                let path = path_arg(args, &self.forge_root)?;
                let indices = hunk_indices_arg(args)?;
                h.with(move |e| e.stage_hunks(&path, &indices)).map_err(map_err)?;
                Ok(json!({"ok": true}))
            }
            HANDLER_UNSTAGE_HUNKS => {
                let path = path_arg(args, &self.forge_root)?;
                let indices = hunk_indices_arg(args)?;
                h.with(move |e| e.unstage_hunks(&path, &indices)).map_err(map_err)?;
                Ok(json!({"ok": true}))
            }
            HANDLER_DISCARD_HUNKS => {
                let path = path_arg(args, &self.forge_root)?;
                let indices = hunk_indices_arg(args)?;
                h.with(move |e| e.discard_hunks(&path, &indices)).map_err(map_err)?;
                Ok(json!({"ok": true}))
            }
            HANDLER_STASH_PUSH => {
                let message = args.get("message").and_then(|v| v.as_str()).map(str::to_string);
                let idx = h.with(move |e| e.stash_push(message.as_deref())).map_err(map_err)?;
                Ok(json!({"ok": true, "index": idx}))
            }
            HANDLER_STASH_LIST => {
                let entries = h.with(|e| e.stash_list()).map_err(map_err)?;
                let arr: Vec<_> = entries
                    .iter()
                    .map(|s| json!({"index": s.index, "message": s.message, "oid": s.oid}))
                    .collect();
                Ok(serde_json::Value::Array(arr))
            }
            HANDLER_STASH_POP => {
                let idx = args.get("index").and_then(|v| v.as_u64())
                    .and_then(|n| usize::try_from(n).ok())
                    .unwrap_or(0);
                h.with(move |e| e.stash_pop(idx)).map_err(map_err)?;
                Ok(json!({"ok": true}))
            }
            HANDLER_STASH_DROP => {
                let idx = args.get("index").and_then(|v| v.as_u64())
                    .and_then(|n| usize::try_from(n).ok())
                    .unwrap_or(0);
                h.with(move |e| e.stash_drop(idx)).map_err(map_err)?;
                Ok(json!({"ok": true}))
            }
            HANDLER_LIST_TAGS => {
                let tags = h.with(|e| e.list_tags()).map_err(map_err)?;
                let arr: Vec<_> = tags
                    .iter()
                    .map(|t| json!({
                        "name":         t.name,
                        "target_hash":  t.target_hash,
                        "is_annotated": t.is_annotated,
                        "message":      t.message,
                    }))
                    .collect();
                Ok(serde_json::Value::Array(arr))
            }
            HANDLER_CREATE_TAG => {
                let name = string_arg(args, "name")?;
                let message = args.get("message").and_then(|v| v.as_str()).map(str::to_string);
                h.with(move |e| e.create_tag(&name, message.as_deref())).map_err(map_err)?;
                Ok(json!({"ok": true}))
            }
            HANDLER_DELETE_TAG => {
                let name = string_arg(args, "name")?;
                h.with(move |e| e.delete_tag(&name)).map_err(map_err)?;
                Ok(json!({"ok": true}))
            }
            HANDLER_PUSH_TAGS => {
                let remote = string_arg(args, "remote")?;
                h.with(move |e| e.push_tags(&remote)).map_err(map_err)?;
                Ok(json!({"ok": true}))
            }
            HANDLER_LFS_STATUS => Ok(lfs_status_snapshot(&self.forge_root)),
            HANDLER_REBASE => {
                let onto = string_arg(args, "onto")?;
                let r = h.with(move |e| e.rebase(&onto)).map_err(map_err)?;
                Ok(json!({
                    "commits_rebased": r.commits_rebased,
                    "conflicts": r.conflicts,
                }))
            }
            HANDLER_ABORT_REBASE => {
                h.with(|e| e.abort_rebase()).map_err(map_err)?;
                Ok(json!({"ok": true}))
            }
            HANDLER_CHERRY_PICK => {
                let commit = string_arg(args, "commit")?;
                let r = h.with(move |e| e.cherry_pick(&commit)).map_err(map_err)?;
                Ok(json!({
                    "commit_hash": r.commit_hash,
                    "conflicts": r.conflicts,
                }))
            }
            HANDLER_ABORT_CHERRY_PICK => {
                h.with(|e| e.abort_cherry_pick()).map_err(map_err)?;
                Ok(json!({"ok": true}))
            }
            HANDLER_CONFLICT_FILES => {
                let files = h.with(|e| e.conflict_files()).map_err(map_err)?;
                Ok(json!({"files": files}))
            }
            HANDLER_ABORT_MERGE => {
                h.with(|e| e.abort_merge()).map_err(map_err)?;
                Ok(json!({"ok": true}))
            }
            HANDLER_CONFLICT_VERSIONS => {
                let path = string_arg(args, "path")?;
                let v = h
                    .with(move |e| e.conflict_versions(&path))
                    .map_err(map_err)?;
                // Bytes go over the wire as JSON arrays of u8 — the
                // shell decodes to a Uint8Array, then to text or
                // binary preview as appropriate.
                Ok(json!({
                    "base":   v.base,
                    "ours":   v.ours,
                    "theirs": v.theirs,
                }))
            }
            HANDLER_MERGE => {
                let branch = string_arg(args, "branch")?;
                let r = h.with(move |e| e.merge(&branch)).map_err(map_err)?;
                Ok(json!({
                    "fast_forward": r.fast_forward,
                    "conflicts":    r.conflicts,
                    "commit_hash":  r.commit_hash,
                }))
            }
            HANDLER_BLAME => {
                // BL-079 — wraps `BlameEntry` into the wire-mirror
                // `GitBlameEntry`. The impl type doesn't derive
                // `Serialize` and carries a `chrono::DateTime` we
                // need to render as ISO-8601 for the shell side.
                let path = path_arg(args, &self.forge_root)?;
                let entries = h.with(move |e| e.blame(&path)).map_err(map_err)?;
                let arr: Vec<_> = entries
                    .iter()
                    .map(|e| {
                        json!({
                            "commit_hash": e.commit_hash,
                            "author": e.author,
                            "date": e.date.to_rfc3339(),
                            "message": e.message,
                            "start_line": e.start_line,
                            "end_line": e.end_line,
                        })
                    })
                    .collect();
                Ok(serde_json::Value::Array(arr))
            }
            _ => Err(PluginError::ExecutionFailed {
                plugin_id: PLUGIN_ID.to_string(),
                reason: format!("unknown handler_id {handler_id}"),
            }),
        }
    }
}

/// BL-091 — snapshot of Git-LFS state for `lfs_status`.
#[doc(hidden)]
pub fn lfs_status_for_forge(forge_root: &Path) -> serde_json::Value {
    lfs_status_snapshot(forge_root)
}


///
/// Inspects `<forge>/.gitattributes` for `filter=lfs` rules and (if
/// the `git-lfs` binary is on `PATH`) shells out to `git lfs
/// ls-files --json`-style output to classify tracked files as
/// pointer-only vs locally-materialised. Designed to be robust to
/// `git-lfs` being absent: in that case `git_lfs_installed = false`,
/// `tracked_patterns` is still populated from `.gitattributes`, and
/// the file lists are empty (signalling "we know LFS is in use here
/// but cannot inspect availability").
fn lfs_status_snapshot(forge_root: &Path) -> serde_json::Value {
    let tracked_patterns = read_lfs_patterns(forge_root);
    let git_lfs_installed = std::process::Command::new("git")
        .args(["lfs", "version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .current_dir(forge_root)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let (pointer_files, available_files) = if git_lfs_installed {
        match std::process::Command::new("git")
            .args(["lfs", "ls-files"])
            .current_dir(forge_root)
            .output()
        {
            Ok(o) if o.status.success() => parse_lfs_ls_files(&o.stdout),
            Ok(o) => {
                tracing::warn!(
                    stderr = %String::from_utf8_lossy(&o.stderr),
                    "BL-091: `git lfs ls-files` exited non-zero",
                );
                (Vec::new(), Vec::new())
            }
            Err(e) => {
                tracing::warn!(error = %e, "BL-091: failed to spawn `git lfs ls-files`");
                (Vec::new(), Vec::new())
            }
        }
    } else {
        (Vec::new(), Vec::new())
    };

    json!({
        "tracked_patterns": tracked_patterns,
        "pointer_files": pointer_files,
        "available_files": available_files,
        "git_lfs_installed": git_lfs_installed,
    })
}

/// Read `<forge>/.gitattributes` and pull out any pattern that
/// declares `filter=lfs`. Lines without the LFS filter are
/// skipped. Missing file → empty list.
fn read_lfs_patterns(forge_root: &Path) -> Vec<String> {
    let path = forge_root.join(".gitattributes");
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.contains("filter=lfs") {
            continue;
        }
        // Pattern is the first whitespace-delimited token.
        if let Some(pat) = line.split_whitespace().next() {
            out.push(pat.to_string());
        }
    }
    out
}

/// Parse the textual output of `git lfs ls-files`. Each line is
/// `<oid> <flag> <path>`, where `flag` is `*` for fully-resolved
/// objects and `-` for pointer-only entries. The format is stable
/// across recent git-lfs versions; if it changes the helper
/// degrades to empty output (still safe — caller treats missing
/// data as "unknown availability").
fn parse_lfs_ls_files(stdout: &[u8]) -> (Vec<String>, Vec<String>) {
    let text = String::from_utf8_lossy(stdout);
    let mut pointers = Vec::new();
    let mut available = Vec::new();
    for line in text.lines() {
        // Split on whitespace into at most three pieces — the path
        // can contain spaces so we keep it as-is.
        let mut parts = line.splitn(3, char::is_whitespace);
        let _oid = parts.next();
        let flag = parts.next();
        let path = parts.next();
        match (flag, path) {
            (Some("*"), Some(p)) => available.push(p.trim().to_string()),
            (Some("-"), Some(p)) => pointers.push(p.trim().to_string()),
            _ => continue,
        }
    }
    (pointers, available)
}

/// Defense-in-depth path validation for git IPC handlers (issue #85).
///
/// libgit2 also rejects `..` and absolute paths inside
/// `index.add_path` / `status_file`, so the trivial traversal is
/// blocked at the libgit2 boundary today. Calling
/// [`resolve_within`](nexus_types::paths::resolve_within) here moves
/// the rejection to the IPC boundary so:
/// - the contract is explicit (a future libgit2 update can't
///   regress this),
/// - the rejection happens before any libgit2 work runs,
/// - the error class is a clean `ExecutionFailed` rather than a
///   generic libgit2 string failure.
///
/// libgit2's path-based API takes a path relative to the repo root,
/// so we discard the joined absolute path and return the validated
/// raw relpath.
fn path_arg(args: &serde_json::Value, forge_root: &Path) -> Result<PathBuf, PluginError> {
    let raw = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: "missing 'path' argument".to_string(),
        })?;
    nexus_types::paths::resolve_within(forge_root, raw).map_err(|e| {
        PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!("invalid 'path': {e}"),
        }
    })?;
    Ok(PathBuf::from(raw))
}

/// Extract the `hunk_indices` array from IPC args as `Vec<usize>`.
fn hunk_indices_arg(args: &serde_json::Value) -> Result<Vec<usize>, PluginError> {
    let arr = args
        .get("hunk_indices")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: "missing 'hunk_indices' array argument".to_string(),
        })?;
    arr.iter()
        .map(|v| {
            v.as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| PluginError::ExecutionFailed {
                    plugin_id: PLUGIN_ID.to_string(),
                    reason: "hunk_indices entries must be non-negative integers".to_string(),
                })
        })
        .collect()
}

/// Extract a plain string argument from the IPC args object.
fn string_arg(args: &serde_json::Value, key: &str) -> Result<String, PluginError> {
    args.get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!("missing '{key}' argument"),
        })
}

// Passed as a function pointer to `.map_err(map_err)`; wrapping in a
// closure would re-trip `redundant_closure`.
#[allow(clippy::needless_pass_by_value)]
fn map_err(e: GitError) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: e.to_string(),
    }
}

// `run_poller` is spawned as a thread and takes ownership of handle,
// bus, and stop for the thread's lifetime — the lint fires because the
// body only needs &handle / &bus / &stop, but hoisting the moves into
// the caller would duplicate the thread::spawn boilerplate.
#[allow(clippy::needless_pass_by_value)]
fn run_poller(handle: GitWorkerHandle, bus: Option<Arc<EventBus>>, stop: Arc<AtomicBool>) {
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
        while waited < POLL_INTERVAL {
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
        publish_git_activity(
            bus,
            "commit",
            &curr.head_oid,
            curr.branch.as_deref(),
        );
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
        ActivityEntry, ActivityOrigin, ActivityOutcome, ActivitySurface,
        ACTIVITY_APPENDED_TOPIC,
    };
    let mut entry = ActivityEntry::now(
        head.to_string(),
        ActivitySurface::Git,
        ActivityOrigin::Git,
    );
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

const AC_TICK: Duration = Duration::from_secs(30);

#[allow(clippy::needless_pass_by_value)]
fn run_auto_committer(
    forge_root: PathBuf,
    idle_secs: u64,
    bus: Option<Arc<EventBus>>,
    stop: Arc<AtomicBool>,
) {
    let mut committer = AutoCommitter::new(&forge_root, 0); // debounce handled externally
    let idle = Duration::from_secs(idle_secs);
    let mut last_modified: Option<Instant> = None;

    let mut sub = bus.as_ref().map(|b| {
        b.subscribe(EventFilter::CustomPrefix("com.nexus.storage.file_modified".to_string()))
    });

    loop {
        if stop.load(Ordering::Relaxed) { break; }

        // Drain file-modified events — each one refreshes the idle timer.
        if let Some(ref mut s) = sub {
            loop {
                match s.try_recv() {
                    Ok(Some(_)) => { last_modified = Some(Instant::now()); }
                    Ok(None) | Err(_) => break,
                }
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
                                ActivityEntry, ActivityOrigin, ActivityOutcome,
                                ActivitySurface, ActivityToolCall,
                                ACTIVITY_APPENDED_TOPIC,
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
                                let _ = b.publish_plugin(
                                    PLUGIN_ID,
                                    ACTIVITY_APPENDED_TOPIC,
                                    payload,
                                );
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
        while waited < AC_TICK {
            if stop.load(Ordering::Relaxed) { return; }
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
            nexus_kernel::NexusEvent::Custom { type_id, payload, .. } => {
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
            if let nexus_kernel::NexusEvent::Custom { type_id, payload, .. } = &ev.event {
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
            if let nexus_kernel::NexusEvent::Custom { type_id, payload, .. } = &ev.event {
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
