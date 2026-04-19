//! Tauri managed state holder for the Nexus kernel runtime.
//!
//! The shell boots with no kernel — `KernelRuntime::new()` yields an empty
//! slot (`None` inside the mutex). When the user picks a workspace via the
//! launcher, a later Tauri command (planned: `boot_kernel`) will call
//! `nexus_bootstrap::build_*` and swap the resulting [`Runtime`] into the
//! slot. Subsequent `kernel_invoke` / `kernel_subscribe` commands then read
//! the slot and route through `Runtime::context.ipc_call(...)`.
//!
//! This module intentionally exposes no Tauri commands yet — it is pure
//! scaffolding for Phase 0 step 2 of the shell↔kernel bridge plan (see
//! `docs/shell-kernel-bridge-plan.md`). The next agent wires `init_forge`
//! and `boot_kernel` on top of this state.

use std::sync::Arc;

use nexus_bootstrap::Runtime;
use tokio::sync::Mutex;

/// Tauri-managed holder for the (optionally-booted) kernel runtime.
///
/// The inner `Option<Runtime>` starts as `None` and is populated by
/// `boot_kernel` once a forge root is known. We use `tokio::sync::Mutex`
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
}

impl Default for KernelRuntime {
    fn default() -> Self {
        Self::new()
    }
}
