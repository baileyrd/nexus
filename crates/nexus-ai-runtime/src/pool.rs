//! Dedicated multi-thread tokio runtime owned by the AI runtime
//! plugin. Long-running LLM rounds run here so they can't starve the
//! host runtime serving UI / IPC traffic.
//!
//! The pool also publishes a [`PoolMetrics`] handle so the
//! `pool_stats` IPC handler can read worker count + max concurrency
//! without needing to introspect tokio internals (tokio doesn't
//! expose a stable "active task count" API).

use std::sync::{Arc, OnceLock};

use tokio::runtime::{Builder, Handle, Runtime};

/// Process-wide cell holding the runtime's pool handle once the
/// `com.nexus.ai.runtime` plugin has been wired. Filled exactly once
/// by [`WorkerPool::publish_shared_handle`] inside the plugin's
/// `wire_context`; consulted by sibling subsystems (notably
/// `nexus-ai::indexing_daemon`) via [`shared_pool_handle`] so they
/// can avoid building a second tokio runtime per ADR 0028.
///
/// `OnceLock` rather than `OnceCell` because the read-side
/// (`shared_pool_handle`) is hit from a worker thread that needs
/// `Sync`. Module-private so the only path to set is via
/// `publish_shared_handle`, which the plugin's `wire_context` owns.
static SHARED_POOL_HANDLE: OnceLock<Handle> = OnceLock::new();

/// Read the runtime's shared tokio runtime handle, if the
/// `com.nexus.ai.runtime` plugin has been wired in this process.
///
/// Returns `None` when (a) the plugin isn't registered (e.g.
/// `nexus-cli` running without bootstrap) or (b) the plugin is
/// registered but `wire_context` hasn't run yet. Callers that take
/// the `None` path should fall back to a process-local Runtime and
/// log so the misordering is observable; see
/// `nexus-ai::indexing_daemon` for the canonical consumer.
///
/// The handle is `Clone` and cheap to copy across worker tasks.
#[must_use]
pub fn shared_pool_handle() -> Option<Handle> {
    SHARED_POOL_HANDLE.get().cloned()
}

/// How many worker threads the pool spawns. Mirrors ADR 0028's
/// `worker_threads = max(2, num_cpus() / 2)` rule of thumb. We use
/// [`std::thread::available_parallelism`] (stable since 1.59) instead
/// of pulling `num_cpus`.
fn default_worker_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| (n.get() / 2).max(2))
        .unwrap_or(2)
}

/// Snapshot of pool dimensioning metrics. Inflight queue / running
/// counts come from [`crate::scheduler::Store`] — this struct only
/// covers what the pool itself owns.
#[derive(Debug, Clone, Copy)]
pub struct PoolMetrics {
    /// Number of worker threads in the dedicated runtime.
    pub workers: u32,
}

/// Owning handle for the pool.
///
/// The tokio [`Runtime`] is dropped off the calling thread (see
/// [`Drop for WorkerPool`]) because `Runtime::Drop` blocks to join its
/// worker threads, and tokio panics if that blocking join runs inside
/// an async context. The field is an [`Option`] so `Drop` can move the
/// `Runtime` out and hand it to a dedicated OS thread.
pub struct WorkerPool {
    /// `Some` for the pool's whole life; taken by `Drop` so the
    /// `Runtime` can be torn down on a non-async thread.
    runtime: Option<Arc<Runtime>>,
    metrics: PoolMetrics,
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        // A tokio `Runtime` must never be *dropped* from within an async
        // context: `Runtime::Drop` blocks to join its worker threads,
        // and tokio panics ("Cannot drop a runtime in a context where
        // blocking is not allowed") when that blocking happens on a
        // runtime worker thread. During kernel teardown this `Drop` runs
        // on the host runtime's worker thread (the ai-runtime plugin is
        // dropped from inside an async shutdown task), so we move the
        // sole `Arc<Runtime>` onto a dedicated OS thread and let the
        // blocking join complete there.
        //
        // The thread is detached: at a forge switch it finishes the join
        // in the background; at process exit the OS reclaims it. We took
        // the `Arc` out of the field, so the helper thread is the only
        // owner — the last-reference drop (and its blocking join) is
        // therefore guaranteed to run off any tokio worker thread.
        let Some(runtime) = self.runtime.take() else {
            return;
        };
        if let Err(e) = std::thread::Builder::new()
            .name("nexus-ai-pool-shutdown".to_string())
            .spawn(move || drop(runtime))
        {
            // Spawn failed (thread exhaustion): `runtime` was dropped by
            // the failed `spawn`, i.e. torn down inline on this thread.
            // That only risks the original panic under the rare combo of
            // thread-exhaustion *and* being on a tokio worker thread —
            // strictly no worse than the pre-fix unconditional drop.
            tracing::warn!(
                error = ?e,
                "nexus-ai-runtime: pool-shutdown thread spawn failed; runtime dropped inline",
            );
        }
    }
}

impl WorkerPool {
    /// Build the pool. `worker_threads` defaults to
    /// [`default_worker_threads`] when `None`.
    ///
    /// # Errors
    /// Returns the underlying [`std::io::Error`] when the tokio
    /// runtime fails to start (only happens on resource exhaustion).
    pub fn start(worker_threads: Option<usize>) -> std::io::Result<Self> {
        let workers = worker_threads.unwrap_or_else(default_worker_threads);
        let runtime = Builder::new_multi_thread()
            .worker_threads(workers)
            .enable_all()
            .thread_name_fn(|| {
                use std::sync::atomic::{AtomicUsize, Ordering};
                static N: AtomicUsize = AtomicUsize::new(0);
                let n = N.fetch_add(1, Ordering::Relaxed);
                format!("nexus-ai-worker-{n}")
            })
            .build()?;
        Ok(Self {
            runtime: Some(Arc::new(runtime)),
            metrics: PoolMetrics {
                workers: u32::try_from(workers).unwrap_or(u32::MAX),
            },
        })
    }

    /// Borrow the runtime handle so callers can `spawn` onto the
    /// dedicated executor. The handle is `Clone` and cheap to copy
    /// across worker tasks.
    ///
    /// # Panics
    /// Only if called after `Drop` has taken the runtime — impossible
    /// for a live `WorkerPool`, since `Drop` is the sole `take()` site.
    #[must_use]
    pub fn handle(&self) -> tokio::runtime::Handle {
        self.runtime
            .as_ref()
            .expect("WorkerPool runtime present for the pool's lifetime")
            .handle()
            .clone()
    }

    /// Snapshot the immutable pool dimensions. Live inflight counts
    /// come from [`crate::scheduler::Store::count_status`], not from
    /// here.
    #[must_use]
    pub fn metrics(&self) -> PoolMetrics {
        self.metrics
    }

    /// BL-134 Phase 4 — publish this pool's handle into the process-
    /// wide [`SHARED_POOL_HANDLE`] so other in-tree subsystems (e.g.
    /// `nexus-ai::indexing_daemon`) can dispatch background work
    /// onto the shared executor without spinning up a second tokio
    /// runtime per ADR 0028's "the runtime is the only consumer that
    /// constructs a dedicated tokio Runtime" rule.
    ///
    /// Idempotent: subsequent calls are no-ops because `OnceLock`
    /// rejects re-sets. Returns `true` when this call installed the
    /// handle, `false` when a previous call already won — useful for
    /// the plugin to log a warn if the lifecycle gets confused.
    pub fn publish_shared_handle(&self) -> bool {
        SHARED_POOL_HANDLE.set(self.handle()).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_shared_handle_installs_a_runtime_handle() {
        // The OnceLock is process-global and shared across the whole
        // test binary — we can't run two `publish_shared_handle`-
        // exercising tests in the same process without contaminating
        // each other. So this test claims the cell; subsequent calls
        // to `publish_shared_handle` (from other tests, or from the
        // plugin's wire_context if it runs in the same process) are
        // expected to be no-ops, which is the documented contract.
        // The test asserts both branches: the installed branch
        // returns true, and `shared_pool_handle()` returns Some after
        // the install.
        let pool = WorkerPool::start(Some(2)).expect("pool starts");
        let installed_first_time = pool.publish_shared_handle();
        // If the OnceLock was already filled by a previously-run test
        // in this binary, `installed_first_time` is false — that's
        // still a valid "shared handle is reachable" outcome.
        assert!(super::shared_pool_handle().is_some());
        if installed_first_time {
            // A subsequent publish must be rejected; the OnceLock
            // can only be set once.
            assert!(
                !pool.publish_shared_handle(),
                "second publish must be a no-op",
            );
        }
    }

    #[test]
    fn pool_starts_with_at_least_two_workers() {
        let pool = WorkerPool::start(None).expect("pool starts");
        assert!(pool.metrics().workers >= 2);
    }

    #[test]
    fn pool_honours_explicit_worker_thread_count() {
        let pool = WorkerPool::start(Some(3)).expect("pool starts");
        assert_eq!(pool.metrics().workers, 3);
    }

    #[test]
    fn handle_can_spawn_a_simple_future() {
        let pool = WorkerPool::start(Some(2)).expect("pool starts");
        let handle = pool.handle();
        let result = handle.block_on(async { 7_u32 + 35 });
        assert_eq!(result, 42);
    }

    #[test]
    fn dropping_pool_inside_async_context_does_not_panic() {
        // Regression: the pool's `Runtime` must not be dropped on a
        // tokio worker thread — `Runtime::Drop` blocks to join workers,
        // which tokio rejects inside an async context with "Cannot drop
        // a runtime in a context where blocking is not allowed". This
        // mirrors kernel teardown, where the ai-runtime plugin (and its
        // pool) is dropped from inside an async shutdown task. Pre-fix
        // this `drop(pool)` panicked the `block_on` thread; with the
        // `Drop` offload it returns normally.
        let outer = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("outer runtime builds");
        outer.block_on(async {
            let pool = WorkerPool::start(Some(2)).expect("pool starts");
            // Touch the handle so the pool is genuinely live before drop.
            let _ = pool.handle();
            drop(pool); // would panic on this worker thread pre-fix
        });
    }
}
