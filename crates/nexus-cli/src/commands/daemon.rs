//! `nexus daemon` — headless, long-running host for workflow triggers
//! (C76).
//!
//! Every workflow trigger engine (cron, `file_event`, `git_event`,
//! `mcp_event`, digest — see `crates/nexus-workflow/src/core_plugin.rs`)
//! is a background tokio task spawned during `build_cli_runtime`'s
//! synchronous plugin wiring. Each one checks
//! `tokio::runtime::Handle::try_current()` and silently disables itself
//! if no runtime is live on the calling thread — true for every CLI
//! one-shot (`App::runtime()` never enters its own tokio runtime before
//! building) and, until C76, also true for `nexus serve` and the TUI
//! (fixed separately in `commands/serve.rs` / `nexus-tui/src/app.rs`).
//!
//! But a one-shot CLI command has no fix available: even a perfectly
//! ordered runtime would just spawn triggers onto a runtime that's
//! dropped the moment the process exits a few milliseconds later. This
//! module is the actual fix for headless/server use: a dedicated,
//! long-running verb that builds the runtime inside a live tokio
//! context and then blocks — on Ctrl+C or SIGTERM (systemd-friendly) —
//! until told to stop, gracefully unloading every plugin (firing every
//! `on_stop` hook, mirroring `shell/src-tauri/src/bridge.rs`'s
//! `KernelRuntime::shutdown`) before exiting.

use std::path::Path;

use anyhow::{Context, Result};
use nexus_bootstrap::build_cli_runtime;

use crate::app::App;

/// C76 — build the tokio runtime and, inside it, the CLI runtime with
/// every workflow trigger engine armed. Split out of [`run`] so the
/// ordering fix itself — entering the tokio runtime *before*
/// `build_cli_runtime`'s synchronous plugin wiring — is unit-testable
/// independent of the blocking Ctrl+C/SIGTERM wait.
///
/// # Errors
/// Returns an error if the tokio runtime fails to start or the CLI
/// runtime fails to build.
fn build_and_arm(forge_root: &Path) -> Result<(tokio::runtime::Runtime, nexus_bootstrap::Runtime)> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .max_blocking_threads(nexus_types::constants::KERNEL_BLOCKING_POOL_SIZE)
        .enable_all()
        .build()
        .context("failed to start tokio runtime")?;

    // C76 — enter *before* building so the workflow plugin's trigger
    // engines see a live `Handle::try_current()` during
    // `build_cli_runtime`'s synchronous wiring pass and actually spawn.
    let guard = rt.enter();
    let runtime = build_cli_runtime(forge_root.to_path_buf())
        .with_context(|| format!("failed to build runtime at '{}'", forge_root.display()))?;
    drop(guard);

    Ok((rt, runtime))
}

/// Gracefully tear `runtime` down: shut the kernel down, then unload
/// every plugin in reverse registration order (firing every `on_stop`
/// hook — the theme watcher thread, workflow's trigger tasks, etc.),
/// mirroring `shell/src-tauri/src/bridge.rs`'s `KernelRuntime::shutdown`.
///
/// `unload` runs *outside* `rt.block_on` deliberately: it's sync, but
/// some plugins (e.g. ai-runtime's worker pool) own a nested tokio
/// `Runtime` that panics on drop if dropped from inside another
/// runtime's async context ("Cannot drop a runtime in a context where
/// blocking is not allowed") — confirmed by hand against a real
/// ai-runtime-registered forge before landing this ordering.
fn graceful_shutdown(rt: &tokio::runtime::Runtime, runtime: nexus_bootstrap::Runtime) {
    rt.block_on(async {
        if let Err(e) = runtime.kernel.shutdown().await {
            eprintln!("nexus daemon: kernel shutdown: {e:#}");
        }
    });
    let mut ids = runtime.loader.lock().registration_order();
    ids.reverse();
    for id in ids {
        if let Err(e) = runtime.loader.lock().unload(&id) {
            eprintln!("nexus daemon: unload {id}: {e}");
        }
    }
}

/// Run the headless daemon at `app`'s forge, blocking until Ctrl+C or
/// SIGTERM.
///
/// # Errors
/// Returns an error if `app` points at a remote (`ssh://`) forge (the
/// trigger engines and every plugin they'd drive live entirely
/// server-side already — there is nothing local to host), the tokio
/// runtime fails to start, or the forge runtime fails to build.
pub fn run(app: &App) -> Result<()> {
    if app.is_remote() {
        anyhow::bail!(
            "nexus daemon requires a local forge; a remote (ssh://) forge already runs its \
             triggers server-side"
        );
    }
    let forge_root = app.forge_root().to_path_buf();
    let (rt, runtime) = build_and_arm(&forge_root)?;

    println!(
        "nexus daemon: triggers armed at '{}'. Press Ctrl+C to stop.",
        forge_root.display()
    );

    rt.block_on(wait_for_stop_signal());

    println!("nexus daemon: shutting down...");
    graceful_shutdown(&rt, runtime);
    println!("nexus daemon: stopped.");
    Ok(())
}

/// Wait for Ctrl+C (SIGINT) or, on Unix, SIGTERM — the signal systemd
/// sends by default when stopping a service.
async fn wait_for_stop_signal() {
    #[cfg(unix)]
    {
        let mut sigterm = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "nexus daemon: failed to install SIGTERM handler; Ctrl+C only");
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn scratch_forge() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        nexus_storage::StorageEngine::init(dir.path()).expect("init scratch forge");
        dir
    }

    fn write_workflow(root: &Path, relpath: &str, body: &str) {
        let abs = root.join(".workflows").join(relpath);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(abs, body).unwrap();
    }

    /// C76's actual bug, proven end-to-end: `build_and_arm` (called from
    /// plain sync code, exactly like `run` calls it — *not* from inside
    /// an already-async `#[tokio::test]`, which would mask the ordering
    /// bug by giving `Handle::try_current()` a runtime for free) must
    /// leave the workflow plugin's `file_event` trigger actually armed.
    /// Mirrors `nexus-bootstrap/tests/workflow_ipc.rs`'s
    /// `file_event_trigger_fires_workflow_when_watched_path_changes`
    /// end-to-end shape (storage watcher → kernel bus →
    /// file_event listener → `run` → a step whose effect proves the
    /// whole loop closed) but through this module's specific
    /// sync-context setup path.
    #[test]
    fn build_and_arm_leaves_file_event_triggers_armed() {
        const WF: &str = r#"
[workflow]
name = "OnFileCreate"

[trigger]
type = "file_event"
watch_dir = "notes/"
pattern = "\\.md$"
events = ["created", "modified"]

[[steps]]
name = "mark"
type = "ipc"
target = "com.nexus.storage"
command = "write_file"
[steps.args]
path = "fired.marker"
bytes = [70, 73, 82, 69, 68]
"#;
        let forge = scratch_forge();
        write_workflow(forge.path(), "onfile.workflow.toml", WF);

        let (rt, runtime) = build_and_arm(forge.path()).expect("build_and_arm");

        rt.block_on(async {
            // Give the storage watcher and the workflow's file_event
            // task a tick to arm before dropping the triggering file.
            tokio::time::sleep(Duration::from_millis(300)).await;

            let notes_dir = forge.path().join("notes");
            std::fs::create_dir_all(&notes_dir).unwrap();
            std::fs::write(notes_dir.join("observed.md"), b"hello").unwrap();

            let marker = forge.path().join("fired.marker");
            let deadline = Instant::now() + Duration::from_secs(8);
            while Instant::now() < deadline {
                if marker.exists() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            assert!(
                marker.exists(),
                "file_event trigger never fired — build_and_arm did not actually arm it"
            );
        });

        graceful_shutdown(&rt, runtime);
    }

    #[test]
    fn graceful_shutdown_does_not_panic_on_a_forge_with_no_workflows() {
        // Regression guard for the "Cannot drop a runtime in a context
        // where blocking is not allowed" panic: unload() must run
        // outside rt.block_on so plugins owning a nested tokio Runtime
        // (ai-runtime's worker pool) can drop cleanly.
        let forge = scratch_forge();
        let (rt, runtime) = build_and_arm(forge.path()).expect("build_and_arm");
        graceful_shutdown(&rt, runtime);
    }
}
