//! Tauri managed state holder for the Nexus kernel runtime + the Tauri
//! commands that boot / tear it down.
//!
//! The shell boots with no kernel — `KernelRuntime::new()` yields an empty
//! slot (`None` inside the mutex). Three commands drive its lifecycle:
//!
//! * [`init_forge`] — prepare an on-disk forge (mkdir + SQLite schema).
//! * [`boot_kernel`] — run `nexus_bootstrap::build_cli_runtime` and swap the
//!   resulting [`Runtime`] into the slot.
//! * [`shutdown_kernel`] — drain the runtime (kernel + plugin loader).
//!
//! On window close, `lib.rs` fires `shutdown_kernel` via
//! [`KernelRuntime::shutdown`] so background tokio tasks and SQLite handles
//! don't leak across restarts.
//!
//! Frontend-facing `kernel_invoke` / `kernel_subscribe` commands land in a
//! follow-up task (Phase 0 step 4+).

use std::path::PathBuf;
use std::sync::Arc;

use nexus_bootstrap::Runtime;
use tokio::sync::Mutex;

// Invoker plugin id note: we currently call
// `nexus_bootstrap::build_cli_runtime`, which registers the invoker as
// `com.nexus.cli`. The private `build(forge_root, invoker_id, invoker_name)`
// is the only place that parameter is configurable, and we're not modifying
// nexus-bootstrap in this task — so the shell rides on the CLI identity until
// a `build_shell_runtime` entry is added upstream.

/// Tauri-managed holder for the (optionally-booted) kernel runtime.
///
/// The inner `Option<Runtime>` starts as `None` and is populated by
/// [`boot_kernel`] once a forge root is known. We use `tokio::sync::Mutex`
/// rather than `std::sync::Mutex` because kernel-bound commands run on
/// Tauri's tokio runtime and hold the guard across await points.
pub struct KernelRuntime {
    inner: Arc<Mutex<Option<Runtime>>>,
}

impl KernelRuntime {
    /// Create an empty runtime slot. No kernel is booted yet.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }

    /// Returns `true` once a `Runtime` has been swapped into the slot.
    ///
    /// Uses `try_lock` so callers can poll cheaply; a contended lock is
    /// treated as "not booted yet" which is the safest default — the only
    /// thing that ever holds this lock for a meaningful duration is the
    /// boot path itself.
    pub fn is_booted(&self) -> bool {
        self.inner
            .try_lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    /// Drain the runtime: pull it out of the slot, call
    /// `kernel.shutdown().await`, then `loader.shutdown()`.
    ///
    /// Idempotent — shutting down an empty slot returns `Ok(())`.
    ///
    /// Errors from individual steps are aggregated so a failing plugin
    /// doesn't prevent the kernel's shutdown flag from flipping or prevent
    /// other plugins from unloading. The joined error string is returned at
    /// the end if any step failed.
    pub async fn shutdown(&self) -> Result<(), String> {
        let runtime_opt = {
            let mut guard = self.inner.lock().await;
            guard.take()
        };
        let Some(runtime) = runtime_opt else {
            return Ok(());
        };

        let mut errors: Vec<String> = Vec::new();

        if let Err(e) = runtime.kernel.shutdown().await {
            errors.push(format!("kernel shutdown: {e:#}"));
        }

        // `PluginLoader` has no `shutdown` — only `PluginManager::shutdown`
        // does, and nexus-bootstrap does not wrap its loader in a manager.
        // Replicate the manager's drain-in-reverse-registration-order logic
        // here so every plugin's `on_stop` hook fires and a failing plugin
        // doesn't prevent its siblings from unloading.
        {
            let mut loader_guard = runtime.loader.lock();
            let mut ids = loader_guard.registration_order();
            ids.reverse();
            for id in ids {
                if let Err(e) = loader_guard.unload(&id) {
                    errors.push(format!("unload {id}: {e}"));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

impl Default for KernelRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// Prepare an on-disk forge at `path`.
///
/// Creates `notes/`, `attachments/`, and `.forge/` (idempotent via
/// `create_dir_all`) then calls [`nexus_bootstrap::init_forge`] which runs
/// `StorageEngine::init` to set up the SQLite schema + search index.
///
/// Does NOT boot a kernel — the caller is expected to follow up with
/// [`boot_kernel`]. Safe to call against an already-initialised forge.
#[tauri::command]
pub async fn init_forge(path: String) -> Result<(), String> {
    let root = PathBuf::from(&path);
    // Mirrors `crates/nexus-app/src/forge.rs::init_layout` ordering: create
    // the top-level skeleton first so `init_forge` below never trips over
    // a missing `.forge/` parent.
    for sub in ["notes", "attachments", ".forge"] {
        let dir = root.join(sub);
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("failed to create '{}': {e}", dir.display()))?;
    }
    nexus_bootstrap::init_forge(&root).map_err(|e| format!("{e:#}"))?;
    Ok(())
}

/// Boot a kernel runtime rooted at `path` and stash it in managed state.
///
/// Rejects with `"kernel already booted"` if a runtime is already in the
/// slot. Callers must shut the old one down first via [`shutdown_kernel`]
/// before booting against a different forge — keeping the two operations
/// explicit avoids hidden re-entrancy during a workspace swap.
///
/// On any error mid-boot the slot stays `None`: `build_cli_runtime` either
/// succeeds and returns a `Runtime`, or fails and leaves nothing behind to
/// clean up.
#[tauri::command]
pub async fn boot_kernel(
    path: String,
    runtime: tauri::State<'_, KernelRuntime>,
) -> Result<(), String> {
    let mut guard = runtime.inner.lock().await;
    if guard.is_some() {
        return Err("kernel already booted".to_string());
    }

    let forge_root = PathBuf::from(&path);
    // `nexus_bootstrap::build` is private; `build_cli_runtime` is the public
    // entry that forwards to `build(forge_root, "com.nexus.cli", "Nexus
    // CLI")`. Using it here means the shell's invoker identity is
    // `com.nexus.cli` for now — acceptable during Phase 0 since no kernel
    // plugin gates on invoker id. A dedicated `build_shell_runtime` can be
    // added to nexus-bootstrap later.
    let built =
        nexus_bootstrap::build_cli_runtime(forge_root).map_err(|e| format!("{e:#}"))?;
    *guard = Some(built);
    Ok(())
}

/// Tear down the kernel runtime if one is booted. Idempotent.
#[tauri::command]
pub async fn shutdown_kernel(
    runtime: tauri::State<'_, KernelRuntime>,
) -> Result<(), String> {
    runtime.shutdown().await
}
