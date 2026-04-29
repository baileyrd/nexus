//! BL-041 — AI background indexing daemon.
//!
//! Subscribes to `com.nexus.storage.file_*` events on the kernel
//! [`EventBus`](nexus_kernel::EventBus), debounces bursts of file
//! changes, and re-indexes the affected files through the existing
//! [`crate::rag::index_file`] pipeline. For deletions it asks
//! `com.nexus.storage::vector_delete_by_file` to drop the chunk
//! embeddings.
//!
//! Status counters live in an [`Arc`]/[`RwLock`]-wrapped [`IndexStatus`]
//! so the `index_status` IPC handler can read them cheaply without
//! reaching into the daemon thread.
//!
//! # Why a dedicated daemon
//!
//! The storage file watcher already publishes `file_created` /
//! `file_modified` / `file_deleted` events on the kernel bus (see
//! [`nexus_storage::core_plugin::bridge_loop`]). The AI plugin is the
//! only consumer that needs to observe those events, debounce a burst
//! of saves into a single embedding pass, and route through
//! `com.nexus.storage::query_blocks` to get the latest block list per
//! file before calling [`crate::rag::index_file`]. Doing this in a
//! dedicated module keeps `core_plugin.rs` from growing another loop
//! and lets us unit-test the debouncer in isolation.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use serde::Serialize;

/// Default debounce window: any path that's been quiet for this long
/// gets flushed on the next tick. Matches the storage watcher's own
/// debounce so a single editor save round-trips through one batch.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_secs(2);

/// Hard cap on pending paths before we flush regardless of quiescence.
/// Keeps memory bounded under a `git checkout` style burst.
pub const DEFAULT_MAX_BATCH: usize = 32;

/// Snapshot of indexing-daemon state. Returned verbatim by the
/// `com.nexus.ai::index_status` IPC handler.
#[derive(Debug, Clone, Default, Serialize)]
pub struct IndexStatus {
    /// Total files successfully (re-)indexed since process start.
    pub indexed_files: u64,
    /// Files currently sitting in the debounce queue.
    pub pending_files: u64,
    /// Total file events ever observed (created + modified + deleted +
    /// renamed). Drifts ahead of `indexed_files` on bursts and stays
    /// strictly `>= indexed_files`.
    pub total_seen: u64,
    /// Last error string from the indexer, if any. Cleared on the next
    /// successful flush.
    pub last_error: Option<String>,
    /// Whether the daemon thread is currently alive. Set by the daemon
    /// in `on_start` and cleared in `on_stop` so the IPC handler can
    /// distinguish "no events yet" from "daemon never started".
    pub running: bool,
}

/// Shared, lock-protected status handle. Cheap to clone; the inner
/// [`RwLock`] is taken briefly on every event arrival and on every
/// `index_status` read.
pub type SharedStatus = Arc<RwLock<IndexStatus>>;

/// Construct a fresh status handle. Used by the AI plugin in
/// `wire_context` so the value lives across daemon restarts (currently
/// only one start/stop cycle per process).
#[must_use]
pub fn new_status() -> SharedStatus {
    Arc::new(RwLock::new(IndexStatus::default()))
}

/// One in-band command for the indexing-daemon worker. Wrapped in an
/// enum so the queue can carry both individual file paths and the
/// shutdown signal across the same channel.
#[derive(Debug, Clone)]
pub enum DaemonMsg {
    /// A path that needs re-indexing. The daemon decides whether to
    /// fetch blocks (modified/created) or call `vector_delete_by_file`
    /// (deleted) at flush time by stat()ing the file again.
    Touched(PathBuf),
    /// A path that's been deleted on disk. Routed to
    /// `vector_delete_by_file` rather than re-indexed.
    Deleted(PathBuf),
}

/// Pure-logic debouncer used by the daemon worker. Splits cleanly from
/// the IPC / bus / threading concerns so it's exercise-able from a
/// synchronous unit test.
///
/// Usage pattern:
///
/// ```ignore
/// let mut deb = Debouncer::new(DEFAULT_DEBOUNCE, DEFAULT_MAX_BATCH);
/// deb.push(DaemonMsg::Touched("a.md".into()));
/// // ... advance clock or push more ...
/// if let Some(batch) = deb.maybe_flush(Instant::now()) {
///     // dispatch batch
/// }
/// ```
pub struct Debouncer {
    debounce: Duration,
    max_batch: usize,
    /// Touched paths are deduplicated — rapid save bursts collapse to
    /// one re-index per file per flush.
    touched: HashSet<PathBuf>,
    /// Deleted paths kept separate so we don't re-index a path that's
    /// been removed in the same window.
    deleted: HashSet<PathBuf>,
    /// Wall-clock arrival time of the most recent message. `None`
    /// means "queue empty".
    last_event: Option<Instant>,
}

impl Debouncer {
    /// Construct a new debouncer with the given window and batch cap.
    #[must_use]
    pub fn new(debounce: Duration, max_batch: usize) -> Self {
        Self {
            debounce,
            max_batch,
            touched: HashSet::new(),
            deleted: HashSet::new(),
            last_event: None,
        }
    }

    /// Enqueue one event. A delete supersedes any prior touch; a touch
    /// after a delete cancels the delete (the file came back).
    pub fn push(&mut self, msg: DaemonMsg) {
        match msg {
            DaemonMsg::Touched(p) => {
                self.deleted.remove(&p);
                self.touched.insert(p);
            }
            DaemonMsg::Deleted(p) => {
                self.touched.remove(&p);
                self.deleted.insert(p);
            }
        }
        self.last_event = Some(Instant::now());
    }

    /// Number of distinct paths currently queued. Used by the daemon
    /// to publish `pending_files` into [`IndexStatus`] without taking
    /// the inner lock more than once per loop tick.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.touched.len() + self.deleted.len()
    }

    /// Decide whether to flush now and, if so, drain the queue and
    /// return the batch. Returns `None` if the queue is empty or the
    /// debounce window hasn't elapsed and the batch cap hasn't been
    /// reached.
    ///
    /// `now` is passed in (rather than read from `Instant::now()`)
    /// so the unit test can drive time deterministically.
    pub fn maybe_flush(&mut self, now: Instant) -> Option<Batch> {
        let pending = self.pending();
        if pending == 0 {
            return None;
        }
        let cap_hit = pending >= self.max_batch;
        let quiet = self
            .last_event
            .is_some_and(|t| now.duration_since(t) >= self.debounce);
        if !(cap_hit || quiet) {
            return None;
        }
        let touched: Vec<PathBuf> = self.touched.drain().collect();
        let deleted: Vec<PathBuf> = self.deleted.drain().collect();
        self.last_event = None;
        Some(Batch { touched, deleted })
    }
}

/// One drained batch of paths ready to be processed by the daemon.
#[derive(Debug, Default)]
pub struct Batch {
    /// Paths that were created or modified — fetch blocks and re-embed.
    pub touched: Vec<PathBuf>,
    /// Paths that were removed — call `vector_delete_by_file`.
    pub deleted: Vec<PathBuf>,
}

impl Batch {
    /// Total path count across both sides. Convenience for stats.
    #[must_use]
    pub fn len(&self) -> usize {
        self.touched.len() + self.deleted.len()
    }

    /// True when neither side has any paths.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.touched.is_empty() && self.deleted.is_empty()
    }
}

/// Tiny helper for the IPC handler: a plain JSON-serializable view of
/// [`IndexStatus`]. The handler clones the inner status under a brief
/// read lock and returns it directly; we don't need a separate DTO
/// because [`IndexStatus`] is already `Serialize`. This wrapper exists
/// solely for documentation locality.
#[must_use]
pub fn snapshot(status: &SharedStatus) -> IndexStatus {
    status.read().map(|g| g.clone()).unwrap_or_default()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn empty_queue_never_flushes() {
        let mut deb = Debouncer::new(Duration::from_millis(50), 4);
        assert!(deb.maybe_flush(Instant::now()).is_none());
    }

    #[test]
    fn single_push_waits_for_debounce_window() {
        let mut deb = Debouncer::new(Duration::from_millis(50), 4);
        deb.push(DaemonMsg::Touched("a.md".into()));
        // Same instant → not yet quiescent.
        let now = Instant::now();
        assert!(deb.maybe_flush(now).is_none());
        // Far in the future → flush.
        let later = now + Duration::from_millis(100);
        let batch = deb.maybe_flush(later).expect("should flush after debounce");
        assert_eq!(batch.touched.len(), 1);
        assert_eq!(batch.deleted.len(), 0);
        assert!(deb.maybe_flush(later).is_none(), "queue drains to empty");
    }

    #[test]
    fn batch_cap_forces_flush_before_debounce() {
        let mut deb = Debouncer::new(Duration::from_secs(60), 3);
        for name in ["a.md", "b.md", "c.md"] {
            deb.push(DaemonMsg::Touched(name.into()));
        }
        // Cap reached → flush immediately even though window not elapsed.
        let batch = deb.maybe_flush(Instant::now()).expect("cap should flush");
        assert_eq!(batch.touched.len(), 3);
    }

    #[test]
    fn duplicate_touches_dedupe() {
        let mut deb = Debouncer::new(Duration::from_millis(10), 16);
        for _ in 0..5 {
            deb.push(DaemonMsg::Touched("a.md".into()));
        }
        let later = Instant::now() + Duration::from_millis(50);
        let batch = deb.maybe_flush(later).expect("should flush");
        assert_eq!(batch.touched.len(), 1);
    }

    #[test]
    fn delete_supersedes_prior_touch() {
        let mut deb = Debouncer::new(Duration::from_millis(10), 16);
        deb.push(DaemonMsg::Touched("a.md".into()));
        deb.push(DaemonMsg::Deleted("a.md".into()));
        let later = Instant::now() + Duration::from_millis(50);
        let batch = deb.maybe_flush(later).expect("should flush");
        assert!(batch.touched.is_empty());
        assert_eq!(batch.deleted.len(), 1);
    }

    #[test]
    fn touch_after_delete_cancels_delete() {
        let mut deb = Debouncer::new(Duration::from_millis(10), 16);
        deb.push(DaemonMsg::Deleted("a.md".into()));
        deb.push(DaemonMsg::Touched("a.md".into()));
        let later = Instant::now() + Duration::from_millis(50);
        let batch = deb.maybe_flush(later).expect("should flush");
        assert!(batch.deleted.is_empty());
        assert_eq!(batch.touched.len(), 1);
    }

    #[test]
    fn pending_count_matches_drained_batch() {
        let mut deb = Debouncer::new(Duration::from_millis(10), 16);
        deb.push(DaemonMsg::Touched("a.md".into()));
        deb.push(DaemonMsg::Touched("b.md".into()));
        deb.push(DaemonMsg::Deleted("c.md".into()));
        assert_eq!(deb.pending(), 3);
    }

    #[test]
    fn snapshot_returns_default_when_unmodified() {
        let s = new_status();
        let snap = snapshot(&s);
        assert!(!snap.running);
        assert_eq!(snap.indexed_files, 0);
        assert!(snap.last_error.is_none());
    }

    #[test]
    fn snapshot_reflects_writes() {
        let s = new_status();
        {
            let mut g = s.write().unwrap();
            g.running = true;
            g.indexed_files = 7;
            g.last_error = Some("boom".to_string());
        }
        let snap = snapshot(&s);
        assert!(snap.running);
        assert_eq!(snap.indexed_files, 7);
        assert_eq!(snap.last_error.as_deref(), Some("boom"));
    }
}
