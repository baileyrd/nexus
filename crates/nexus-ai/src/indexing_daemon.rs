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
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, RwLock,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use nexus_kernel::{EventFilter, Events as _, Ipc as _, KernelPluginContext, NexusEvent};
use serde::Serialize;
use tokio::sync::mpsc;

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

/// Block tuple shape consumed by [`crate::rag::index_file`]:
/// `(block_id, kind, text, position)`. Exposed as a type alias so the
/// `decode_blocks` signature stays under clippy's `type_complexity`
/// threshold.
pub type BlockTuple = (u64, String, String, Option<i32>);

/// Type alias for the embedder factory closure handed to
/// [`IndexingDaemon::start`]. Boxed + `Send + Sync + 'static` because
/// the daemon thread re-invokes it for every batch to pick up runtime
/// `set_config` changes.
pub type EmbedderFactory =
    Arc<dyn Fn() -> Option<Box<dyn crate::embedding::EmbeddingProvider>> + Send + Sync + 'static>;

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

/// IPC plugin id of the storage core plugin. Inlined to avoid a circular
/// crate dependency on `nexus-storage`.
pub(crate) const STORAGE_PLUGIN_ID: &str = "com.nexus.storage";
/// IPC command name for `query_blocks` on the storage plugin.
const CMD_QUERY_BLOCKS: &str = "query_blocks";
/// IPC command name for `vector_delete_by_file` on the storage plugin.
const CMD_VECTOR_DELETE_BY_FILE: &str = "vector_delete_by_file";
/// Custom event prefix the storage bridge publishes file events under.
const STORAGE_EVENT_PREFIX: &str = "com.nexus.storage.file_";

/// Per-IPC call timeout. Indexing is a background task — we don't want
/// a hung HTTP embedding request to block the daemon thread forever,
/// but 30s is plenty of headroom for a local fastembed batch.
const IPC_TIMEOUT: Duration = Duration::from_secs(30);

/// Convert a storage `path` JSON field (relative to the forge root) into
/// a [`PathBuf`]. Filters obvious non-markdown paths so we don't waste
/// embedding cycles on, say, `.png` thumbnails dropped into the forge.
fn parse_storage_path(payload: &serde_json::Value) -> Option<PathBuf> {
    let s = payload.get("path")?.as_str()?;
    let p = PathBuf::from(s);
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
    // Storage watcher already filters to `.md` / `.markdown`, but the
    // bridge re-emits everything; double-check here for safety.
    if matches!(ext, "md" | "markdown") {
        Some(p)
    } else {
        None
    }
}

/// Translate a `PublishedEvent` from the bus into a [`DaemonMsg`], if
/// it's a storage file event we care about. Returns `None` for events
/// from other plugins (cheaper than `EventFilter::CustomPrefix` because
/// we sidestep the per-event filter allocation).
fn event_to_msg(event: &NexusEvent) -> Vec<DaemonMsg> {
    let NexusEvent::Custom {
        type_id, payload, ..
    } = event
    else {
        return Vec::new();
    };
    if !type_id.starts_with(STORAGE_EVENT_PREFIX) {
        return Vec::new();
    }
    match type_id.as_str() {
        "com.nexus.storage.file_created" | "com.nexus.storage.file_modified" => {
            parse_storage_path(payload)
                .map(|p| vec![DaemonMsg::Touched(p)])
                .unwrap_or_default()
        }
        "com.nexus.storage.file_deleted" => parse_storage_path(payload)
            .map(|p| vec![DaemonMsg::Deleted(p)])
            .unwrap_or_default(),
        "com.nexus.storage.file_renamed" => {
            // FU-4 — emit delete-old + touch-new. The storage payload
            // exposes both via `from` / `to`. Splitting at translation
            // time means the debouncer correctly dedupes the new path
            // against any subsequent `file_modified` and the old
            // path's vectors are reaped via `vector_delete_by_file`
            // without waiting for the next save on it.
            let mut out = Vec::with_capacity(2);
            if let Some(p) = payload
                .get("from")
                .and_then(serde_json::Value::as_str)
                .and_then(|s| parse_storage_path(&serde_json::json!({ "path": s })))
            {
                out.push(DaemonMsg::Deleted(p));
            }
            if let Some(p) = payload
                .get("to")
                .and_then(serde_json::Value::as_str)
                .and_then(|s| parse_storage_path(&serde_json::json!({ "path": s })))
            {
                out.push(DaemonMsg::Touched(p));
            }
            out
        }
        _ => Vec::new(),
    }
}

/// Owning handle for a running indexing daemon. Construct via
/// [`IndexingDaemon::start`]; drop or call [`IndexingDaemon::stop`] to
/// shut down. The daemon owns its own current-thread tokio runtime so
/// it can call `ctx.ipc_call` (an async method) without depending on
/// the frontend's runtime topology.
pub struct IndexingDaemon {
    /// Channel out to the worker thread. `None` after `stop()`.
    msg_tx: Option<mpsc::UnboundedSender<DaemonMsg>>,
    /// Set to `true` when the worker should drain & exit.
    shutdown: Arc<AtomicBool>,
    /// Joined on `stop()`. `None` after the join completes.
    handle: Option<JoinHandle<()>>,
    /// Shared status snapshot — handed back via [`status_handle`].
    status: SharedStatus,
}

impl IndexingDaemon {
    /// Borrow the shared status handle so the IPC handler can read it.
    #[must_use]
    pub fn status_handle(&self) -> SharedStatus {
        Arc::clone(&self.status)
    }

    /// Spawn the indexing daemon. The worker thread:
    ///
    /// 1. Subscribes to the kernel bus via `ctx.subscribe(...)` and
    ///    forwards matching storage file events into the debouncer.
    /// 2. Wakes every 200 ms to call [`Debouncer::maybe_flush`]; on
    ///    flush, drives the per-path indexing through `ctx.ipc_call`.
    ///
    /// `ctx` must hold the `IpcCall` capability and be the AI plugin's
    /// kernel context (so `query_blocks` / `vector_delete_by_file`
    /// resolve correctly).
    ///
    /// `embedder_factory` is invoked once per batch to materialise an
    /// [`EmbeddingProvider`]. Returning `None` from the factory means
    /// "no embedding provider configured" — the batch is skipped and
    /// `last_error` is set. The factory shape (rather than holding the
    /// provider directly) lets the daemon pick up runtime config
    /// changes pushed via `set_config`.
    ///
    /// # Errors
    /// Returns the underlying [`std::io::Error`] if the worker thread
    /// fails to spawn (e.g. resource exhaustion). The status handle is
    /// updated with `running = true` even on success of *spawn*; the
    /// inner runtime build failure (vanishingly rare) is logged and
    /// folded into `last_error` from inside the worker.
    pub fn start(
        ctx: Arc<KernelPluginContext>,
        status: SharedStatus,
        embedder_factory: EmbedderFactory,
    ) -> std::io::Result<Self> {
        Self::start_with_debounce(ctx, status, embedder_factory, DEFAULT_DEBOUNCE)
    }

    /// P2-06 — same as [`Self::start`] plus an explicit debounce
    /// window. Use when the AiConfig override is in play; otherwise
    /// the no-arg form falls through to [`DEFAULT_DEBOUNCE`].
    ///
    /// # Errors
    /// Same as [`Self::start`].
    pub fn start_with_debounce(
        ctx: Arc<KernelPluginContext>,
        status: SharedStatus,
        embedder_factory: EmbedderFactory,
        debounce: Duration,
    ) -> std::io::Result<Self> {
        let (msg_tx, msg_rx) = mpsc::unbounded_channel::<DaemonMsg>();
        let shutdown = Arc::new(AtomicBool::new(false));

        // Mark "running" before the thread spawns so a fast subsequent
        // status read sees the daemon as alive.
        if let Ok(mut g) = status.write() {
            g.running = true;
        }

        let shutdown_for_thread = Arc::clone(&shutdown);
        let status_for_thread = Arc::clone(&status);

        let handle = std::thread::Builder::new()
            .name("nexus-ai-indexing-daemon".to_string())
            .spawn(move || {
                // BL-134 Phase 4 — prefer the `nexus-ai-runtime` shared
                // pool handle when available so the daemon dispatches
                // its async work onto the runtime's multi-thread
                // executor instead of building its own current-thread
                // runtime. Falls back when the runtime plugin isn't
                // registered (e.g. test harnesses that boot
                // `nexus-ai` without the full bootstrap) or hasn't
                // wired its context yet — the per-bootstrap ordering
                // pins ai-runtime BEFORE ai so the typical-path is
                // the shared-handle branch.
                if let Some(shared) = nexus_ai_runtime::shared_pool_handle() {
                    tracing::info!(
                        thread = "nexus-ai-indexing-daemon",
                        "BL-134 Phase 4: dispatching indexing loop onto the shared ai-runtime pool",
                    );
                    shared.block_on(worker_loop(
                        ctx,
                        msg_rx,
                        shutdown_for_thread,
                        status_for_thread.clone(),
                        embedder_factory,
                        debounce,
                    ));
                } else {
                    tracing::warn!(
                        thread = "nexus-ai-indexing-daemon",
                        "BL-134 Phase 4: ai-runtime shared pool handle not available; \
                         falling back to a dedicated current-thread runtime. \
                         This path indicates the ai-runtime plugin isn't wired \
                         (e.g. unit-test bootstrap, or a regression in plugin order).",
                    );
                    let rt = match tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        Ok(rt) => rt,
                        Err(e) => {
                            tracing::error!(?e, "indexing daemon: failed to build runtime");
                            if let Ok(mut g) = status_for_thread.write() {
                                g.running = false;
                                g.last_error = Some(format!("runtime build failed: {e}"));
                            }
                            return;
                        }
                    };
                    rt.block_on(worker_loop(
                        ctx,
                        msg_rx,
                        shutdown_for_thread,
                        status_for_thread.clone(),
                        embedder_factory,
                        debounce,
                    ));
                }
                if let Ok(mut g) = status_for_thread.write() {
                    g.running = false;
                }
            })?;

        Ok(Self {
            msg_tx: Some(msg_tx),
            shutdown,
            handle: Some(handle),
            status,
        })
    }

    /// Push a raw [`DaemonMsg`] onto the queue. Used by the tests; in
    /// production the daemon's own bus subscription is the only sender.
    pub fn enqueue(&self, msg: DaemonMsg) {
        if let Some(tx) = self.msg_tx.as_ref() {
            let _ = tx.send(msg);
        }
    }

    /// Clone the queue's sender so an external caller (e.g. the
    /// `index_trigger` IPC handler — FU-2) can fan a forge walk into
    /// the existing debouncer without holding `&self` across an
    /// async await. Returns `None` after [`Self::stop`] has been
    /// called.
    #[must_use]
    pub fn sender_handle(&self) -> Option<mpsc::UnboundedSender<DaemonMsg>> {
        self.msg_tx.clone()
    }

    /// Signal shutdown and join the worker. Idempotent.
    pub fn stop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // Drop the sender so the worker's `recv()` returns `None` and
        // breaks out of the inner select arm even before the 200 ms
        // tick elapses.
        self.msg_tx.take();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        if let Ok(mut g) = self.status.write() {
            g.running = false;
        }
    }
}

impl Drop for IndexingDaemon {
    fn drop(&mut self) {
        // Best-effort shutdown so a panicked AI plugin doesn't leak
        // the worker thread.
        if self.handle.is_some() {
            self.stop();
        }
    }
}

/// Async worker loop. Owns the [`Debouncer`] and the tokio mpsc
/// receiver; subscribes to the kernel bus and drives the indexing
/// pipeline on each flush.
async fn worker_loop(
    ctx: Arc<KernelPluginContext>,
    mut msg_rx: mpsc::UnboundedReceiver<DaemonMsg>,
    shutdown: Arc<AtomicBool>,
    status: SharedStatus,
    embedder_factory: EmbedderFactory,
    debounce: Duration,
) {
    let mut sub = ctx.subscribe(EventFilter::CustomPrefix(STORAGE_EVENT_PREFIX.to_string()));
    let mut debouncer = Debouncer::new(debounce, DEFAULT_MAX_BATCH);

    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        // Pump bus events into the debouncer (non-blocking).
        loop {
            match sub.try_recv() {
                Ok(Some(evt)) => {
                    let msgs = event_to_msg(&evt.event);
                    if !msgs.is_empty() {
                        if let Ok(mut g) = status.write() {
                            g.total_seen = g.total_seen.saturating_add(1);
                        }
                        for msg in msgs {
                            debouncer.push(msg);
                        }
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!(?e, "indexing daemon: bus recv error");
                    break;
                }
            }
        }

        // Pump direct mpsc messages (tests / explicit triggers).
        while let Ok(msg) = msg_rx.try_recv() {
            if let Ok(mut g) = status.write() {
                g.total_seen = g.total_seen.saturating_add(1);
            }
            debouncer.push(msg);
        }

        // Update pending count under one brief write lock.
        if let Ok(mut g) = status.write() {
            g.pending_files = u64::try_from(debouncer.pending()).unwrap_or(u64::MAX);
        }

        // Maybe flush.
        if let Some(batch) = debouncer.maybe_flush(Instant::now()) {
            process_batch(ctx.as_ref(), &status, &embedder_factory, batch).await;
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Drive one drained batch through the indexing pipeline. Errors are
/// captured into [`IndexStatus::last_error`] but never abort the
/// daemon — the next batch retries.
async fn process_batch(
    ctx: &KernelPluginContext,
    status: &SharedStatus,
    embedder_factory: &EmbedderFactory,
    batch: Batch,
) {
    // Deletions first — cheap, no embedder needed.
    for path in batch.deleted {
        let path_str = path.to_string_lossy().to_string();
        let res = ctx
            .ipc_call(
                STORAGE_PLUGIN_ID,
                CMD_VECTOR_DELETE_BY_FILE,
                serde_json::json!({ "path": path_str }),
                IPC_TIMEOUT,
            )
            .await;
        match res {
            Ok(_) => {
                if let Ok(mut g) = status.write() {
                    g.indexed_files = g.indexed_files.saturating_add(1);
                    g.last_error = None;
                }
            }
            Err(e) => {
                let msg = format!("vector_delete_by_file({path_str}): {e}");
                tracing::warn!(error = %msg, "indexing daemon: delete failed");
                if let Ok(mut g) = status.write() {
                    g.last_error = Some(msg);
                }
            }
        }
    }

    // Touches — fetch blocks via storage, then run the embed+upsert pass.
    if batch.touched.is_empty() {
        return;
    }
    let Some(embedder) = embedder_factory() else {
        if let Ok(mut g) = status.write() {
            g.last_error = Some("no embedding provider configured".to_string());
        }
        return;
    };

    for path in batch.touched {
        let path_str = path.to_string_lossy().to_string();
        // C28 (#381) — excluded notes are never embedded; reap any
        // chunks indexed before the exclusion was added so a fresh
        // `.aiignore` line / `ai: exclude` frontmatter takes effect on
        // the file's next change event.
        if crate::exclusion::is_excluded(ctx, &path_str).await {
            tracing::debug!(path = %path_str, "C28: excluded from AI indexing");
            let _ = ctx
                .ipc_call(
                    STORAGE_PLUGIN_ID,
                    CMD_VECTOR_DELETE_BY_FILE,
                    serde_json::json!({ "path": path_str }),
                    IPC_TIMEOUT,
                )
                .await;
            continue;
        }
        let blocks_res = ctx
            .ipc_call(
                STORAGE_PLUGIN_ID,
                CMD_QUERY_BLOCKS,
                serde_json::json!({ "path": path_str }),
                IPC_TIMEOUT,
            )
            .await;
        let blocks_value = match blocks_res {
            Ok(v) => v,
            Err(e) => {
                let msg = format!("query_blocks({path_str}): {e}");
                tracing::warn!(error = %msg, "indexing daemon: blocks fetch failed");
                if let Ok(mut g) = status.write() {
                    g.last_error = Some(msg);
                }
                continue;
            }
        };

        // The storage handler returns `Vec<BlockRecord>`. The chunker
        // expects `Vec<BlockTuple>` — same
        // wire shape as the existing `index_file` handler accepts. We
        // re-shape here rather than depend on `nexus-storage`'s
        // `BlockRecord` struct.
        let blocks: Vec<BlockTuple> = match decode_blocks(&blocks_value) {
            Ok(v) => v,
            Err(e) => {
                let msg = format!("decode blocks({path_str}): {e}");
                tracing::warn!(error = %msg, "indexing daemon: decode failed");
                if let Ok(mut g) = status.write() {
                    g.last_error = Some(msg);
                }
                continue;
            }
        };

        match crate::rag::index_file(ctx, embedder.as_ref(), &path_str, &blocks).await {
            Ok(_n) => {
                if let Ok(mut g) = status.write() {
                    g.indexed_files = g.indexed_files.saturating_add(1);
                    g.last_error = None;
                }
            }
            Err(e) => {
                let msg = format!("index_file({path_str}): {e}");
                tracing::warn!(error = %msg, "indexing daemon: index failed");
                if let Ok(mut g) = status.write() {
                    g.last_error = Some(msg);
                }
            }
        }
    }
}

/// Decode `query_blocks`'s JSON return value into the chunker's tuple
/// shape. Accepts either the raw array `[BlockRecord, ...]` or an
/// object wrapper `{ "blocks": [...] }` — both shapes have appeared
/// at the IPC boundary historically. Each record must expose
/// `block_id` (u64), `kind` (string), `text` (string), and
/// (optionally) `position` (i32).
///
/// Type alias [`BlockTuple`] keeps the chunker-facing tuple shape
/// short for clippy's `type_complexity` lint.
///
/// # Errors
/// Returns a string describing the first decode failure encountered:
/// - The outer value isn't an array or `{ blocks: [...] }`.
/// - A record is missing the required `block_id` field.
fn decode_blocks(value: &serde_json::Value) -> Result<Vec<BlockTuple>, String> {
    let arr = value
        .as_array()
        .or_else(|| value.get("blocks").and_then(|v| v.as_array()))
        .ok_or_else(|| "expected array or { blocks: [...] }".to_string())?;
    let mut out = Vec::with_capacity(arr.len());
    for (i, b) in arr.iter().enumerate() {
        let id = b
            .get("block_id")
            .or_else(|| b.get("id"))
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| format!("block[{i}]: missing block_id"))?;
        let kind = b
            .get("kind")
            .or_else(|| b.get("block_type"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("paragraph")
            .to_string();
        let text = b
            .get("text")
            .or_else(|| b.get("content"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        let position = b
            .get("position")
            .and_then(serde_json::Value::as_i64)
            .and_then(|v| i32::try_from(v).ok());
        out.push((id, kind, text, position));
    }
    Ok(out)
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn daemon_observes_storage_event_and_increments_total_seen() {
        use nexus_kernel::{CapabilitySet, EventBus, InMemoryKvStore, KvStore};
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let kv: Arc<dyn KvStore> = Arc::new(InMemoryKvStore::new());
        let bus = Arc::new(EventBus::new(64));
        let ctx = KernelPluginContext::new(
            "com.nexus.ai",
            "0.0.1",
            CapabilitySet::default(),
            kv,
            Arc::clone(&bus),
            dir.path(),
            None,
        )
        .unwrap();

        let status = new_status();
        // Embedder factory returns None — we won't actually flush
        // because no path will satisfy the debounce window in the
        // brief interval, and even if it did, the touch path would
        // bail with "no embedding provider" rather than panic.
        let factory: Arc<
            dyn Fn() -> Option<Box<dyn crate::embedding::EmbeddingProvider>>
                + Send
                + Sync
                + 'static,
        > = Arc::new(|| None);

        let mut daemon =
            IndexingDaemon::start(Arc::new(ctx), Arc::clone(&status), factory).unwrap();

        // Wait until the worker thread has had a chance to subscribe;
        // EventBus::subscribe only delivers events published *after*
        // the receiver was created.
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(25)).await;
            if bus.subscriber_count() >= 1 {
                break;
            }
        }
        assert!(
            bus.subscriber_count() >= 1,
            "daemon failed to subscribe to bus"
        );

        // Publish a storage file_modified event onto the bus.
        bus.publish_plugin(
            "com.nexus.storage",
            "com.nexus.storage.file_modified",
            serde_json::json!({ "path": "notes/foo.md" }),
        )
        .unwrap();

        // Give the worker a few tick cycles to drain.
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if snapshot(&status).total_seen >= 1 {
                break;
            }
        }
        let snap = snapshot(&status);
        assert!(snap.running, "daemon should report running");
        assert!(
            snap.total_seen >= 1,
            "expected total_seen >= 1, got {snap:?}"
        );

        daemon.stop();
        let snap = snapshot(&status);
        assert!(!snap.running, "daemon should clear running on stop");
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
    fn event_to_msg_translates_storage_file_modified() {
        let evt = NexusEvent::Custom {
            type_id: "com.nexus.storage.file_modified".to_string(),
            emitting_plugin: "com.nexus.storage".to_string(),
            payload: serde_json::json!({ "path": "notes/today.md" }),
        };
        let out = event_to_msg(&evt);
        assert_eq!(out.len(), 1);
        match &out[0] {
            DaemonMsg::Touched(p) => assert_eq!(p, &PathBuf::from("notes/today.md")),
            DaemonMsg::Deleted(_) => panic!("expected Touched, got Deleted"),
        }
    }

    #[test]
    fn event_to_msg_translates_storage_file_deleted() {
        let evt = NexusEvent::Custom {
            type_id: "com.nexus.storage.file_deleted".to_string(),
            emitting_plugin: "com.nexus.storage".to_string(),
            payload: serde_json::json!({ "path": "old.md" }),
        };
        let out = event_to_msg(&evt);
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0], DaemonMsg::Deleted(_)));
    }

    #[test]
    fn event_to_msg_skips_non_markdown() {
        let evt = NexusEvent::Custom {
            type_id: "com.nexus.storage.file_modified".to_string(),
            emitting_plugin: "com.nexus.storage".to_string(),
            payload: serde_json::json!({ "path": "asset.png" }),
        };
        assert!(event_to_msg(&evt).is_empty());
    }

    #[test]
    fn event_to_msg_skips_unrelated_events() {
        let evt = NexusEvent::Custom {
            type_id: "com.nexus.theme.reloaded".to_string(),
            emitting_plugin: "com.nexus.theme".to_string(),
            payload: serde_json::json!({}),
        };
        assert!(event_to_msg(&evt).is_empty());
    }

    /// FU-4 — `file_renamed` must split into delete-old + touch-new
    /// so the old path's vectors are reaped at the rename boundary
    /// rather than waiting for the next save on it.
    #[test]
    fn event_to_msg_splits_file_renamed_into_delete_and_touch() {
        let evt = NexusEvent::Custom {
            type_id: "com.nexus.storage.file_renamed".to_string(),
            emitting_plugin: "com.nexus.storage".to_string(),
            payload: serde_json::json!({
                "from": "drafts/a.md",
                "to": "notes/a.md",
            }),
        };
        let out = event_to_msg(&evt);
        assert_eq!(out.len(), 2, "rename must yield two messages, got {out:?}");
        match (&out[0], &out[1]) {
            (DaemonMsg::Deleted(from), DaemonMsg::Touched(to)) => {
                assert_eq!(from, &PathBuf::from("drafts/a.md"));
                assert_eq!(to, &PathBuf::from("notes/a.md"));
            }
            other => panic!("expected (Deleted, Touched), got {other:?}"),
        }
    }

    /// A rename whose `from` is non-markdown still emits the
    /// touch-new (we want the new path indexed); the inverse holds
    /// when `to` is non-markdown (delete the old vectors).
    #[test]
    fn event_to_msg_renamed_filters_non_markdown_per_side() {
        let renamed_to_image = NexusEvent::Custom {
            type_id: "com.nexus.storage.file_renamed".to_string(),
            emitting_plugin: "com.nexus.storage".to_string(),
            payload: serde_json::json!({
                "from": "notes/old.md",
                "to": "assets/old.png",
            }),
        };
        let out = event_to_msg(&renamed_to_image);
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0], DaemonMsg::Deleted(_)));
    }

    #[test]
    fn decode_blocks_accepts_array_form() {
        let v = serde_json::json!([
            { "block_id": 1, "kind": "heading", "text": "# Title", "position": 0 },
            { "block_id": 2, "kind": "paragraph", "text": "body", "position": 1 },
        ]);
        let out = decode_blocks(&v).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, 1);
        assert_eq!(out[0].1, "heading");
        assert_eq!(out[1].2, "body");
    }

    #[test]
    fn decode_blocks_accepts_wrapped_form() {
        let v = serde_json::json!({ "blocks": [
            { "block_id": 7, "kind": "code", "text": "println!(\"hi\");" }
        ]});
        let out = decode_blocks(&v).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, 7);
        assert!(out[0].3.is_none());
    }

    #[test]
    fn decode_blocks_rejects_non_array() {
        assert!(decode_blocks(&serde_json::json!("not an array")).is_err());
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
