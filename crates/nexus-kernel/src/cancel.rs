//! Cooperative IPC cancellation — Track A.
//!
//! Rust offers no safe thread interrupt, so cancellation must be
//! cooperative: the handler voluntarily checks a signal and returns
//! early. This module provides the *signal pipe* and an opt-in
//! handler accessor for picking it up.
//!
//! # Why task-local, not a trait parameter
//!
//! Threading a `CancellationToken` through every `CorePlugin::dispatch`
//! / `dispatch_async` signature would tax the 90 % of handlers that
//! will never check it to serve the 10 % that should. Instead, the
//! kernel sets a `tokio::task_local!` for the duration of the dispatch
//! and exposes [`ipc_cancel_token`]. Handlers that care opt in:
//!
//! ```ignore
//! if let Some(cancel) = nexus_kernel::ipc_cancel_token() {
//!     tokio::select! {
//!         _ = cancel.cancelled() => return Err(/* cancelled */),
//!         result = slow_work() => result,
//!     }
//! } else {
//!     slow_work().await
//! }
//! ```
//!
//! # Wait-side semantics
//!
//! Even without handler opt-in, the kernel races the active token
//! against the dispatch in `context_impl::ipc_call_inner`. When the
//! token fires the kernel returns [`IpcError::Cancelled`] to the
//! caller and drops the handler future / abandons the blocking-pool
//! slot. The handler may keep running (Rust doesn't allow forced
//! cancellation of an in-progress future or `spawn_blocking` body);
//! handlers that hold expensive resources should opt in via
//! [`ipc_cancel_token`] to actually release them.
//!
//! # Outside an IPC dispatch
//!
//! [`ipc_cancel_token`] returns `None` outside an active dispatch
//! (e.g., background scheduler ticks, plugin lifecycle hooks). That
//! is the correct behaviour — there is no caller to be cancelled by
//! — but the `None` arm must be handled by opt-in callers.

// Re-exported so callers that need to name the token type (e.g. the
// Tauri shell's per-window cancel map) can import it from the kernel
// crate instead of pulling in a direct tokio-util dependency.
pub use tokio_util::sync::CancellationToken;

tokio::task_local! {
    /// Active cancellation token for the currently-dispatching IPC
    /// call on this task. Set by the kernel just before invoking the
    /// handler future / spawn_blocking body; absent outside an IPC
    /// dispatch.
    static IPC_CANCEL: CancellationToken;
}

/// Return the cancellation token attached to the currently-running
/// IPC dispatch, if any.
///
/// Returns `None` when called outside an IPC dispatch (background
/// schedulers, lifecycle hooks, tests that drive plugins directly).
/// Handlers that want to opt in to cancellation should test for
/// `Some` and either `select!` against `token.cancelled()` or call
/// `token.is_cancelled()` at convenient yield points.
#[must_use]
pub fn ipc_cancel_token() -> Option<CancellationToken> {
    IPC_CANCEL.try_with(Clone::clone).ok()
}

/// Run `fut` with `token` bound as the active IPC cancellation token
/// for the duration of its execution.
///
/// Called by the kernel dispatch path to install the token before
/// handing control to the handler future. Plugin code should not
/// call this directly — the kernel owns the install side; plugins
/// read via [`ipc_cancel_token`].
pub async fn scope_async<F, T>(token: CancellationToken, fut: F) -> T
where
    F: std::future::Future<Output = T>,
{
    IPC_CANCEL.scope(token, fut).await
}

/// Sync analogue of [`scope_async`] — run `f` with `token` bound as
/// the active IPC cancellation token. Used by the sync-dispatch
/// `spawn_blocking` path; sync handlers that want to participate
/// must poll `token.is_cancelled()` at yield points themselves
/// (Rust offers no preemption inside a blocking call).
pub fn scope_sync<F, T>(token: CancellationToken, f: F) -> T
where
    F: FnOnce() -> T,
{
    IPC_CANCEL.sync_scope(token, f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ipc_cancel_token_returns_none_outside_a_dispatch() {
        assert!(ipc_cancel_token().is_none());
    }

    #[tokio::test]
    async fn ipc_cancel_token_returns_the_scoped_token_inside_a_dispatch() {
        let outer = CancellationToken::new();
        let observed = scope_async(outer.clone(), async {
            ipc_cancel_token().expect("token must be present inside scope")
        })
        .await;
        // The scoped token must be the same token the kernel installed,
        // so a cancel on the outer side is observable to the handler.
        assert!(!observed.is_cancelled());
        outer.cancel();
        assert!(observed.is_cancelled());
    }

    #[tokio::test]
    async fn scope_does_not_leak_token_after_completion() {
        let outer = CancellationToken::new();
        scope_async(outer, async {
            assert!(ipc_cancel_token().is_some());
        })
        .await;
        assert!(
            ipc_cancel_token().is_none(),
            "task-local must clear after scope exits"
        );
    }
}
