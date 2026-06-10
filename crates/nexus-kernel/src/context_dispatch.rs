//! IPC dispatch entry point + blocking-pool instrumentation for
//! [`KernelPluginContext`] (V11, `repo-review-2026-06-10.md` — split
//! out of `context_impl.rs`, which had grown to 27% of the kernel).
//!
//! This child module owns the [`Ipc`] trait impl — the metrics +
//! failure-audit bracketing around `ipc_call_inner`, which stays in
//! the parent next to the capability checks it depends on — and the
//! sync-dispatch blocking-pool observability statics. Declared as a
//! child module of `context_impl` (via `#[path]`), so the context's
//! private fields stay accessible without widening their visibility.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use nexus_types::constants::{KERNEL_BLOCKING_POOL_SIZE, KERNEL_BLOCKING_POOL_WARN_DEPTH};

use super::KernelPluginContext;
use crate::context::Ipc;
use crate::error::IpcError;

#[async_trait]
impl Ipc for KernelPluginContext {
    async fn ipc_call(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        args: serde_json::Value,
        timeout: Duration,
    ) -> std::result::Result<serde_json::Value, IpcError> {
        // BL-093: bracket the entire dispatch with a timer so every
        // exit path records `ipc_calls_total` + `ipc_call_duration`.
        let started = std::time::Instant::now();
        let result = self
            .ipc_call_inner(target_plugin_id, command_id, args, timeout)
            .await;
        let elapsed = started.elapsed();
        if let Some(m) = crate::metrics::global() {
            let status = match &result {
                Ok(_) => crate::metrics::CallStatus::Ok,
                Err(IpcError::CapabilityDenied { .. }) => {
                    crate::metrics::CallStatus::CapabilityDenied
                }
                Err(IpcError::CommandNotFound { .. } | IpcError::PluginNotFound { .. }) => {
                    crate::metrics::CallStatus::NotFound
                }
                Err(IpcError::Timeout { .. }) => crate::metrics::CallStatus::Timeout,
                Err(IpcError::Cancelled { .. }) => crate::metrics::CallStatus::Cancelled,
                _ => crate::metrics::CallStatus::Error,
            };
            m.record_ipc_call(
                target_plugin_id,
                command_id,
                status,
                u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX),
            );
        }
        // Audit gap D3 (`docs/0.1.2/audits/gaps-inconsistencies-2026-05-21.md`).
        // The dispatcher is the only common point that sees every IPC failure;
        // individual handlers used to swallow errors via `?`-propagation with
        // no log line. Severity is tuned per error class:
        //   - CapabilityDenied — already audited via `audit::log_capability_denied`
        //     inside `ipc_call_inner`; skip here to avoid double-logging.
        //   - Cancelled — normal user-initiated tear-down; debug only.
        //   - PluginCrashedDuringCall with an EMPTY reason — a true handler
        //     panic / blocking-task join failure / poisoned lock; elevate to
        //     error.
        //   - PluginCrashedDuringCall with a NON-EMPTY reason — the loader wraps
        //     every handler `Err(PluginError::ExecutionFailed)` in this variant
        //     too, carrying the handler's message as `reason` (the variant doc:
        //     "Empty for true panics"). That's an ordinary handled rejection
        //     (e.g. "no open session", "collab not configured"), not a crash —
        //     log at warn so routine failures don't masquerade as crashes.
        //   - Everything else (Timeout, NotFound, plugin-returned PluginError, …) — warn.
        if let Err(err) = &result {
            let elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
            match err {
                IpcError::CapabilityDenied { .. } => {}
                IpcError::Cancelled { .. } => {
                    tracing::debug!(
                        caller = %self.plugin_id,
                        target = target_plugin_id,
                        command = command_id,
                        elapsed_ms,
                        "ipc_call cancelled",
                    );
                }
                IpcError::PluginCrashedDuringCall { reason, .. } if reason.is_empty() => {
                    tracing::error!(
                        caller = %self.plugin_id,
                        target = target_plugin_id,
                        command = command_id,
                        elapsed_ms,
                        error = %err,
                        "ipc_call: plugin crashed during dispatch",
                    );
                }
                _ => {
                    tracing::warn!(
                        caller = %self.plugin_id,
                        target = target_plugin_id,
                        command = command_id,
                        elapsed_ms,
                        error = %err,
                        "ipc_call failed",
                    );
                }
            }
        }
        result
    }
}

// ── Sync IPC dispatch blocking-pool observability ────────────────────────────

/// In-flight count of sync IPC dispatches currently held on the host
/// tokio runtime's blocking pool. Each `ipc_call` whose target handler
/// is registered as sync (no async impl) increments this counter
/// before `spawn_blocking` and decrements it when the spawned task
/// completes (via a `Drop` guard, so a panic in the handler still
/// returns the slot).
///
/// Bounded by the host runtime's `max_blocking_threads`, which
/// frontends size from [`nexus_types::constants::KERNEL_BLOCKING_POOL_SIZE`].
/// Reads are lock-free (`Ordering::Relaxed`) — the value is advisory,
/// used only for warn-on-high-water and metrics.
static IN_FLIGHT_SYNC_DISPATCHES: AtomicUsize = AtomicUsize::new(0);

/// Latches `true` once the in-flight depth crosses
/// [`KERNEL_BLOCKING_POOL_WARN_DEPTH`] so the operator-visible warn
/// fires once per saturation episode rather than per call. Resets to
/// `false` when depth drops back below half the threshold (hysteresis
/// against thrash near the boundary).
static HIGH_WATER_WARNED: AtomicBool = AtomicBool::new(false);

/// Snapshot of the current in-flight sync IPC dispatch count.
/// Exposed for metrics / debug surfaces; callers must treat the value
/// as a monotonically-stale-by-one read.
#[must_use]
pub fn in_flight_sync_dispatches() -> usize {
    IN_FLIGHT_SYNC_DISPATCHES.load(Ordering::Relaxed)
}

/// Like `tokio::task::spawn_blocking` but instruments the sync IPC
/// dispatch path with an in-flight counter and a one-shot warn when
/// the depth crosses [`KERNEL_BLOCKING_POOL_WARN_DEPTH`]. The
/// counter decrements via a `Drop` guard inside the spawned closure
/// so a handler panic still returns the slot.
pub(super) fn spawn_blocking_sync_dispatch<F, R>(f: F) -> tokio::task::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let depth = IN_FLIGHT_SYNC_DISPATCHES.fetch_add(1, Ordering::Relaxed) + 1;
    if depth >= KERNEL_BLOCKING_POOL_WARN_DEPTH && !HIGH_WATER_WARNED.swap(true, Ordering::Relaxed)
    {
        tracing::warn!(
            audit = true,
            depth,
            warn_threshold = KERNEL_BLOCKING_POOL_WARN_DEPTH,
            pool_cap = KERNEL_BLOCKING_POOL_SIZE,
            "kernel: sync IPC dispatch depth above warn threshold; the host \
             tokio runtime is approaching its max_blocking_threads cap. \
             Consider converting slow handlers to async dispatch."
        );
    }
    tokio::task::spawn_blocking(move || {
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                let prev = IN_FLIGHT_SYNC_DISPATCHES.fetch_sub(1, Ordering::Relaxed);
                // Clear the warn latch on hysteresis — once depth drops
                // below half the threshold the next saturation episode
                // logs again. Avoids both warn-per-call spam and warn-
                // suppression after a single sticky episode.
                if prev.saturating_sub(1) < KERNEL_BLOCKING_POOL_WARN_DEPTH / 2 {
                    HIGH_WATER_WARNED.store(false, Ordering::Relaxed);
                }
            }
        }
        let _guard = Guard;
        f()
    })
}
