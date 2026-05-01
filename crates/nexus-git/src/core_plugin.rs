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
use std::time::Duration;

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, PluginError};
use serde_json::json;

use crate::{GitError, GitState, GitWorker, GitWorkerHandle};

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
            _ => Err(PluginError::ExecutionFailed {
                plugin_id: PLUGIN_ID.to_string(),
                reason: format!("unknown handler_id {handler_id}"),
            }),
        }
    }
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
        };
        let curr = GitState {
            branch: Some("feature".to_string()),
            head_oid: "abc1234".to_string(),
            is_dirty: false,
            repo_state: crate::RepoState::Clean,
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
}
