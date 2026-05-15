//! Dedicated multi-thread tokio runtime owned by the AI runtime
//! plugin. Long-running LLM rounds run here so they can't starve the
//! host runtime serving UI / IPC traffic.
//!
//! The pool also publishes a [`PoolMetrics`] handle so the
//! `pool_stats` IPC handler can read worker count + max concurrency
//! without needing to introspect tokio internals (tokio doesn't
//! expose a stable "active task count" API).

use std::sync::Arc;

use tokio::runtime::{Builder, Runtime};

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

/// Owning handle for the pool. Drop joins all workers (tokio
/// `Runtime::Drop` waits for in-flight tasks unless `shutdown_*` was
/// called explicitly).
pub struct WorkerPool {
    runtime: Arc<Runtime>,
    metrics: PoolMetrics,
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
            runtime: Arc::new(runtime),
            metrics: PoolMetrics {
                workers: u32::try_from(workers).unwrap_or(u32::MAX),
            },
        })
    }

    /// Borrow the runtime handle so callers can `spawn` onto the
    /// dedicated executor. The handle is `Clone` and cheap to copy
    /// across worker tasks.
    #[must_use]
    pub fn handle(&self) -> tokio::runtime::Handle {
        self.runtime.handle().clone()
    }

    /// Snapshot the immutable pool dimensions. Live inflight counts
    /// come from [`crate::scheduler::Store::count_status`], not from
    /// here.
    #[must_use]
    pub fn metrics(&self) -> PoolMetrics {
        self.metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
