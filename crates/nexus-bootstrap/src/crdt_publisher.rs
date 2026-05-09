//! BL-074 editor wiring: a [`nexus_editor::OpObserver`] that mirrors
//! every editor session into a [`nexus_crdt::CrdtDoc`], publishes
//! per-op envelopes to the kernel event bus on
//! `com.nexus.editor.ops.<relpath>`, and persists CRDT state to
//! `<forge>/.forge/.editor/crdt/<sha-of-relpath>.json` on close.
//!
//! The publisher lives in `nexus-bootstrap` (rather than alongside the
//! library in `nexus-crdt` or alongside the trait in `nexus-editor`)
//! because `nexus-crdt` already depends on `nexus-editor`. Putting the
//! glue here lets it pull both crates without forcing a dep cycle.
//!
//! Consumers see two observable side effects after wiring:
//!
//! - On every successful `apply_transaction`, a `com.nexus.editor.ops.
//!   <relpath>` event fires with payload `{ "op": <CrdtOp> }`. Plugins
//!   or test harnesses can subscribe via `EventFilter::CustomPrefix`
//!   to watch ops fly by in real time.
//! - On `close`, the per-relpath `<sha>.json` file appears under
//!   `.forge/.editor/crdt/`. Reopening the same file restores the
//!   CRDT state when the markdown source bytes still hash to the
//!   stored `content_hash`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use nexus_crdt::{
    conflict_topic, content_hash_hex, crdt_state_path, ops_topic, Conflict, ConflictEnvelope,
    CrdtDoc, OpEnvelope, PersistedCrdt, RemoteOutcome, SiteId, VersionVector,
};
use nexus_editor::{BlockTree, OpObserver, Operation, Transaction, EDITOR_PLUGIN_ID};
use nexus_kernel::{EventBus, EventFilter};
use serde_json::Value;

/// Topic the BL-007 pull-landing subscriber listens on. Fired by
/// `nexus-git`'s state poller when HEAD advances — which covers the
/// merge / fast-forward paths a `git pull` ends in. Local commits
/// also fire this; reload is a no-op in that case (live session is
/// already in sync with the file we just wrote).
const PULL_LANDING_TOPIC: &str = "com.nexus.git.commit";

/// Poll cadence for the pull-landing subscriber. Latency from a HEAD
/// advance to a session reload is bounded by this. 250ms keeps idle
/// CPU at zero while still feeling instant in the editor UI.
const PULL_LANDING_TICK: Duration = Duration::from_millis(250);

/// BL-007 pull-landing report. Emitted by
/// [`CrdtPublisher::reload_after_external_change`] after a `git pull`
/// (or any other writer) has updated `.forge/.editor/crdt/<sha>.json`
/// so the caller can log / surface conflicts.
#[derive(Debug, Default)]
pub struct ReloadOutcome {
    /// Number of remote ops applied to the live session via
    /// `apply_remote`. Each one is also published on the ops topic.
    pub absorbed: usize,
    /// Conflicts that surfaced while absorbing remote ops. The Phase 2
    /// silent-merge path resolves text overlap inside `apply_remote`,
    /// so what reaches here is structural-delete-vs-edit and concurrent
    /// whole-block replacements — both need a resolver UI (see
    /// BL-074 follow-ups).
    pub conflicts: Vec<Conflict>,
}

/// Reason a [`CrdtPublisher::reload_after_external_change`] returned
/// [`None`]. Non-fatal — the caller logs at debug and moves on.
#[derive(Debug)]
pub enum ReloadSkip {
    /// The publisher has no open session for this relpath. Reload will
    /// happen naturally on the next `on_session_opened` (which reads
    /// the file directly).
    NoSession,
    /// The persisted state file does not exist. Either the forge
    /// doesn't track CRDT state for this file, or git removed it.
    Missing,
    /// The file exists but couldn't be decoded (version mismatch,
    /// truncated, malformed JSON). Logged at debug; same policy as
    /// `load_persisted` — degrade rather than crash.
    Invalid,
}

/// Default checkpoint cadence: persist every N applied ops while the
/// session is open, on top of the unconditional close-time write.
/// 32 covers ~a few seconds of bursty typing and keeps the on-disk
/// state recoverable to within a small edit window if the process is
/// killed.
pub const DEFAULT_CHECKPOINT_EVERY_OPS: u64 = 32;

/// Per-process site identity. Each running editor process is one CRDT
/// site; popout windows and TUI/CLI flows that share the same backend
/// inherit it. Generated once at construction.
#[derive(Clone, Debug)]
pub struct CrdtPublisher {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    forge_root: PathBuf,
    site: SiteId,
    bus: Arc<EventBus>,
    docs: Mutex<HashMap<String, SessionState>>,
    /// Number of ops between forced checkpoint writes. 0 disables
    /// periodic writes (close-only persistence).
    checkpoint_every: u64,
}

/// Per-relpath state held by the publisher: the live CRDT doc plus a
/// checkpoint counter for periodic-write triggering.
#[derive(Debug)]
struct SessionState {
    doc: CrdtDoc,
    /// Ops applied since the last successful checkpoint write (or
    /// session open). Reset to 0 after each checkpoint.
    ops_since_checkpoint: u64,
    /// BL-074 follow-up — op-log compaction oracle. The doc's version
    /// vector at session open (after any disk-restore) is the cheapest
    /// "this slice has already been written to disk" snapshot we can
    /// take. On close we prune the log down to it: anything in the
    /// open-time VV is older than this session and was already
    /// persisted; ops authored or absorbed during the session sit
    /// above it and survive compaction. Multi-peer forges keep extra
    /// ops around because the open-time VV doesn't include peer ops
    /// gossiped while the session was open — exactly the conservative
    /// trade-off the BL-074 follow-up calls out.
    prune_floor_at_open: VersionVector,
}

impl CrdtPublisher {
    /// Construct a publisher rooted at `forge_root`, publishing on
    /// `bus`. Generates a fresh per-process [`SiteId`] and uses the
    /// [`DEFAULT_CHECKPOINT_EVERY_OPS`] cadence for periodic
    /// persistence.
    #[must_use]
    pub fn new(forge_root: PathBuf, bus: Arc<EventBus>) -> Self {
        Self::with_checkpoint_every(forge_root, bus, DEFAULT_CHECKPOINT_EVERY_OPS)
    }

    /// Construct a publisher with a custom periodic-checkpoint cadence
    /// (in ops). Pass `0` to disable periodic writes — only the
    /// close-time write happens. Tests use `0` to keep on-disk
    /// behavior fully deterministic.
    #[must_use]
    pub fn with_checkpoint_every(
        forge_root: PathBuf,
        bus: Arc<EventBus>,
        checkpoint_every: u64,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                forge_root,
                site: SiteId::new(),
                bus,
                docs: Mutex::new(HashMap::new()),
                checkpoint_every,
            }),
        }
    }

    /// This site's id (for tests / debug surfaces).
    #[must_use]
    pub fn site(&self) -> SiteId {
        self.inner.site
    }

    /// BL-007 helper: relpaths with a live session. Stable order is
    /// not guaranteed (HashMap iteration). Used by the pull-landing
    /// subscriber and by tests.
    #[must_use]
    pub fn open_relpaths(&self) -> Vec<String> {
        match self.inner.docs.lock() {
            Ok(g) => g.keys().cloned().collect(),
            Err(p) => p.into_inner().keys().cloned().collect(),
        }
    }

    /// BL-007 bus wiring. Spawn a background thread that subscribes
    /// to [`PULL_LANDING_TOPIC`] and, for each event observed, calls
    /// [`Self::reload_after_external_change`] for every open relpath.
    /// Returns the [`std::thread::JoinHandle`] so the caller can join
    /// at shutdown if it wants — bootstrap stores it on the runtime
    /// so the thread is part of the orderly teardown chain; tests
    /// just `join()` directly.
    ///
    /// The thread holds a [`Weak`] reference to the publisher's inner
    /// state, so dropping the last [`CrdtPublisher`] clone causes the
    /// next `Weak::upgrade` to fail and the thread to exit cleanly —
    /// no explicit shutdown signal needed.
    ///
    /// Idempotent in spirit (subscribing twice is allowed but
    /// wasteful — bootstrap calls this exactly once per publisher).
    ///
    /// # Panics
    /// Panics if the OS refuses to spawn the subscriber thread, which
    /// is treated as boot-time fatal — bootstrap can't continue
    /// without the pull-landing path.
    #[must_use]
    pub fn start_pull_landing_subscriber(&self) -> std::thread::JoinHandle<()> {
        // Create the subscription on the *parent* thread so any event
        // published after this call is observed. Subscribing inside
        // the spawned thread races: the bus drops events that arrive
        // before any subscriber exists.
        let sub = self
            .inner
            .bus
            .subscribe(EventFilter::CustomExact(PULL_LANDING_TOPIC.to_string()));
        let weak = Arc::downgrade(&self.inner);
        std::thread::Builder::new()
            .name("nexus-crdt-pull-landing".to_string())
            .spawn(move || run_pull_landing_subscriber(weak, sub))
            .expect("spawn nexus-crdt-pull-landing thread")
    }

    fn state_file(&self, relpath: &str) -> PathBuf {
        self.inner.forge_root.join(crdt_state_path(relpath))
    }

    /// Try to restore a doc from disk. Returns `None` and logs at
    /// `debug` level for any of: file missing, version mismatch,
    /// hash mismatch, decode failure. Mirrors the BL-072 undo
    /// invalidation policy — degrade to a fresh doc rather than
    /// surfacing the error.
    fn load_persisted(&self, relpath: &str, source_bytes: &[u8]) -> Option<PersistedCrdt> {
        let path = self.state_file(relpath);
        let bytes = std::fs::read(&path).ok()?;
        let envelope: PersistedCrdt = match serde_json::from_slice(&bytes) {
            Ok(e) => e,
            Err(err) => {
                tracing::debug!(%err, path = %path.display(), "BL-074: persisted crdt decode failed");
                return None;
            }
        };
        if envelope.version != nexus_crdt::PERSISTED_VERSION {
            tracing::debug!(
                version = envelope.version,
                path = %path.display(),
                "BL-074: persisted crdt version mismatch"
            );
            return None;
        }
        if envelope.content_hash != content_hash_hex(source_bytes) {
            tracing::debug!(
                path = %path.display(),
                "BL-074: persisted crdt hash mismatch — markdown changed externally"
            );
            return None;
        }
        Some(envelope)
    }

    fn write_persisted(&self, relpath: &str, doc: &CrdtDoc, source_bytes: &[u8]) {
        self.write_persisted_state(relpath, &doc.state(), source_bytes);
    }

    /// Lower-level form that takes a pre-captured snapshot, used by
    /// the periodic-checkpoint path so the snapshot can be taken
    /// under the session lock and the I/O happens outside it.
    /// Returns `true` if the file was successfully replaced.
    fn write_persisted_state(
        &self,
        relpath: &str,
        state: &nexus_crdt::CrdtState,
        source_bytes: &[u8],
    ) -> bool {
        let path = self.state_file(relpath);
        if let Some(parent) = path.parent() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                tracing::warn!(%err, path = %parent.display(), "BL-074: mkdir failed");
                return false;
            }
        }
        let envelope = PersistedCrdt::new(state.clone(), content_hash_hex(source_bytes));
        let bytes = match serde_json::to_vec(&envelope) {
            Ok(b) => b,
            Err(err) => {
                tracing::warn!(%err, "BL-074: encode persisted crdt failed");
                return false;
            }
        };
        // Atomic write: temp + rename on the same filesystem so a
        // crash mid-write can't leave a half-written file.
        let tmp = path.with_extension("json.tmp");
        if let Err(err) = std::fs::write(&tmp, &bytes) {
            tracing::warn!(%err, path = %tmp.display(), "BL-074: tmp write failed");
            return false;
        }
        if let Err(err) = std::fs::rename(&tmp, &path) {
            tracing::warn!(%err, path = %path.display(), "BL-074: rename failed");
            let _ = std::fs::remove_file(&tmp);
            return false;
        }
        true
    }

    fn publish_op(&self, relpath: &str, op: nexus_crdt::CrdtOp) {
        let envelope = OpEnvelope::new(op);
        let payload: Value = match envelope.to_json() {
            Ok(v) => v,
            Err(err) => {
                tracing::warn!(%err, relpath, "BL-074: envelope encode failed");
                return;
            }
        };
        let topic = ops_topic(relpath);
        if let Err(err) = self.inner.bus.publish_plugin(EDITOR_PLUGIN_ID, &topic, payload) {
            tracing::warn!(%err, relpath, "BL-074: bus publish failed");
        }
    }

    /// BL-007 conflict surface. Publish a [`ConflictEnvelope`] on
    /// [`conflict_topic`] so the shell (or any subscriber) can render
    /// a resolver UI. The CRDT layer can't pick a winner for
    /// `StructuralDeleteEdit` or whole-block-replacement conflicts —
    /// the user must.
    fn publish_conflicts(&self, relpath: &str, conflicts: Vec<Conflict>) {
        if conflicts.is_empty() {
            return;
        }
        let envelope = ConflictEnvelope::new(conflicts);
        let payload: Value = match envelope.to_json() {
            Ok(v) => v,
            Err(err) => {
                tracing::warn!(%err, relpath, "BL-007: conflict envelope encode failed");
                return;
            }
        };
        let topic = conflict_topic(relpath);
        if let Err(err) = self.inner.bus.publish_plugin(EDITOR_PLUGIN_ID, &topic, payload) {
            tracing::warn!(%err, relpath, "BL-007: conflict bus publish failed");
        }
    }

    /// Canonical-markdown bytes for the doc's current tree, used at
    /// close time to compute the integrity hash that goes into the
    /// persistence envelope. Matches what the editor's `save` would
    /// write — same parser/serializer pair.
    fn canonical_markdown_bytes(doc: &CrdtDoc) -> Vec<u8> {
        nexus_editor::MarkdownSerializer::serialize(doc.tree()).into_bytes()
    }
}

impl OpObserver for CrdtPublisher {
    fn on_session_opened(&self, relpath: &str, tree: &BlockTree, source_bytes: &[u8]) {
        let restored = self.load_persisted(relpath, source_bytes);
        let doc = match restored {
            Some(envelope) => {
                tracing::debug!(relpath, "BL-074: restored persisted crdt state");
                CrdtDoc::from_state(tree.clone(), envelope.state)
            }
            None => CrdtDoc::new(self.inner.site, tree.clone()),
        };
        let mut guard = match self.inner.docs.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let prune_floor_at_open = doc.log().version_vector().clone();
        guard.insert(
            relpath.to_string(),
            SessionState {
                doc,
                ops_since_checkpoint: 0,
                prune_floor_at_open,
            },
        );
    }

    fn on_session_closed(&self, relpath: &str) {
        let mut guard = match self.inner.docs.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some(mut state) = guard.remove(relpath) {
            // BL-074 op-log compaction. Anything dominated by the
            // open-time VV was on disk before this session started, so
            // a future peer that loads the persisted state still finds
            // those ids reported as seen via `OpLog::pruned_floor`.
            // Keeps single-replica forges from accumulating an
            // ever-growing log across reopens.
            let pruned = state.doc.compact_to(&state.prune_floor_at_open);
            if pruned > 0 {
                tracing::debug!(relpath, pruned, "BL-074: compacted op log on close");
            }
            let source_bytes = Self::canonical_markdown_bytes(&state.doc);
            self.write_persisted(relpath, &state.doc, &source_bytes);
        }
    }

    fn on_apply_transaction(&self, relpath: &str, ops: &[Operation]) {
        self.apply_local_ops(relpath, ops.iter().cloned());
    }

    fn on_undo_transaction(&self, relpath: &str, reversed: &Transaction, _post_tree: &BlockTree) {
        // Author the inverse of each op (in reverse order) against
        // the publisher's own mirror tree. The mirror is still in the
        // *post-tx* state at this point — we re-derive the inverse
        // from there so byte/annotation positions are accurate.
        let inverses: Vec<Operation> = {
            let guard = match self.inner.docs.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            let Some(state) = guard.get(relpath) else {
                tracing::debug!(relpath, "BL-074: undo with no observed session — dropped");
                return;
            };
            let mirror = state.doc.tree();
            let mut out = Vec::with_capacity(reversed.operations.len());
            for op in reversed.operations.iter().rev() {
                match op.inverse(mirror) {
                    Ok(inv) => out.push(inv),
                    Err(err) => {
                        tracing::warn!(%err, relpath, "BL-074: undo inverse failed; aborting propagation");
                        return;
                    }
                }
            }
            out
        };
        self.apply_local_ops(relpath, inverses.into_iter());
    }

    fn on_redo_transaction(&self, relpath: &str, replayed: &Transaction, _post_tree: &BlockTree) {
        // Redo replays the original ops; from the CRDT's view this is
        // just a fresh local apply.
        self.apply_local_ops(relpath, replayed.operations.iter().cloned());
    }
}

impl CrdtPublisher {
    /// BL-007 pull-landing hook. Re-read the persisted state file for
    /// `relpath` (post `git pull` / merge driver) and absorb any ops
    /// the live session hasn't seen via [`CrdtDoc::apply_remote`].
    /// Each absorbed op is also published on the
    /// `com.nexus.editor.ops.<relpath>` topic so subscribers (editor
    /// UI, plugins) converge.
    ///
    /// Returns `Ok(ReloadOutcome)` on success — `absorbed` may be 0
    /// if the persisted log was already a subset of the live log.
    /// Returns `Err(ReloadSkip)` for the non-fatal cases (no live
    /// session, no state file, decode failure). Unlike
    /// [`Self::load_persisted`], this path does **not** check the
    /// `content_hash` — after a `git pull` the markdown source on
    /// disk also changed, so the live session's baseline source bytes
    /// are stale by design. The op log union remains correct because
    /// it's keyed on [`nexus_crdt::OpId`], not on byte offsets.
    ///
    /// # Errors
    ///
    /// Returns [`ReloadSkip`] in the cases described above. None of
    /// them indicate a bug — they're caller-side telemetry.
    pub fn reload_after_external_change(
        &self,
        relpath: &str,
    ) -> std::result::Result<ReloadOutcome, ReloadSkip> {
        let envelope = self.load_persisted_unchecked(relpath)?;
        let (absorbed_envelopes, conflicts) = {
            let mut guard = match self.inner.docs.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            let Some(state) = guard.get_mut(relpath) else {
                return Err(ReloadSkip::NoSession);
            };
            let mut absorbed = Vec::new();
            let mut conflicts = Vec::new();
            for op in envelope.state.log.iter() {
                if state.doc.log().contains(op.id) {
                    continue;
                }
                let cloned = op.clone();
                match state.doc.apply_remote(cloned.clone()) {
                    Ok(RemoteOutcome::Applied) => absorbed.push(cloned),
                    Ok(RemoteOutcome::Duplicate) => {
                        // Should be filtered by `contains` above, but
                        // `apply_remote` is the source of truth.
                    }
                    Ok(RemoteOutcome::Conflict(c)) => conflicts.push(c),
                    Err(err) => {
                        tracing::warn!(
                            %err,
                            relpath,
                            op_id = ?op.id,
                            "BL-007: apply_remote rejected op during pull-landing reload"
                        );
                    }
                }
            }
            (absorbed, conflicts)
        };
        let absorbed_count = absorbed_envelopes.len();
        for op in absorbed_envelopes {
            self.publish_op(relpath, op);
        }
        // Conflicts also fire on the bus so the shell can render a
        // resolver UI without polling — same contract as absorbed
        // ops on `ops_topic`. The returned `ReloadOutcome` still
        // carries them so imperative callers (tests, future MCP
        // commands) can react without subscribing.
        self.publish_conflicts(relpath, conflicts.clone());
        Ok(ReloadOutcome {
            absorbed: absorbed_count,
            conflicts,
        })
    }

    /// Read and decode the persisted envelope without checking the
    /// content hash — used by the BL-007 pull-landing path where the
    /// markdown source is expected to have changed alongside the
    /// state file.
    fn load_persisted_unchecked(
        &self,
        relpath: &str,
    ) -> std::result::Result<PersistedCrdt, ReloadSkip> {
        let path = self.state_file(relpath);
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(ReloadSkip::Missing);
            }
            Err(err) => {
                tracing::debug!(%err, path = %path.display(), "BL-007: state read failed");
                return Err(ReloadSkip::Invalid);
            }
        };
        let envelope: PersistedCrdt = match serde_json::from_slice(&bytes) {
            Ok(e) => e,
            Err(err) => {
                tracing::debug!(%err, path = %path.display(), "BL-007: state decode failed");
                return Err(ReloadSkip::Invalid);
            }
        };
        if envelope.version != nexus_crdt::PERSISTED_VERSION {
            tracing::debug!(
                version = envelope.version,
                path = %path.display(),
                "BL-007: state version mismatch"
            );
            return Err(ReloadSkip::Invalid);
        }
        Ok(envelope)
    }

    /// Shared body for apply / undo / redo: feeds each op through
    /// `CrdtDoc::apply_local` under the session lock, publishes wire
    /// envelopes outside it, and triggers a periodic checkpoint if
    /// the threshold is hit.
    fn apply_local_ops<I: Iterator<Item = Operation>>(&self, relpath: &str, ops: I) {
        let (wire_ops, checkpoint) = {
            let mut guard = match self.inner.docs.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            let Some(state) = guard.get_mut(relpath) else {
                tracing::debug!(
                    relpath,
                    "BL-074: apply_local_ops with no observed session — dropped"
                );
                return;
            };
            let wire: Vec<_> = ops
                .filter_map(|op| match state.doc.apply_local(&op) {
                    Ok(wire) => Some(wire),
                    Err(err) => {
                        tracing::warn!(%err, relpath, "BL-074: apply_local rejected op");
                        None
                    }
                })
                .collect();
            state.ops_since_checkpoint = state.ops_since_checkpoint.saturating_add(wire.len() as u64);
            let checkpoint = if self.inner.checkpoint_every > 0
                && state.ops_since_checkpoint >= self.inner.checkpoint_every
            {
                // Snapshot the doc *under the lock* so the on-disk
                // image is consistent with a real point in the op
                // sequence. Subsequent apply_locals will queue up
                // until we re-acquire (or never, since we hold no
                // observers' attention here). Reset the counter only
                // if the write succeeds — if it fails the next op
                // gets another shot.
                let snapshot = state.doc.state();
                let source_bytes = Self::canonical_markdown_bytes(&state.doc);
                Some((snapshot, source_bytes))
            } else {
                None
            };
            (wire, checkpoint)
        };
        for wire in wire_ops {
            self.publish_op(relpath, wire);
        }
        if let Some((snapshot, source_bytes)) = checkpoint {
            if self.write_persisted_state(relpath, &snapshot, &source_bytes) {
                // Reset the counter under the lock so a concurrent
                // observer sees the post-checkpoint state.
                if let Ok(mut guard) = self.inner.docs.lock() {
                    if let Some(state) = guard.get_mut(relpath) {
                        state.ops_since_checkpoint = 0;
                    }
                }
            }
        }
    }
}

/// Body of the pull-landing thread spawned by
/// [`CrdtPublisher::start_pull_landing_subscriber`]. Reads from a
/// pre-built [`nexus_kernel::EventSubscription`] and, whenever an
/// event arrives, asks the publisher to reload every open relpath.
/// Exits when the publisher's last strong [`Arc`] drops (so the
/// [`Weak`] no longer upgrades).
fn run_pull_landing_subscriber(weak: Weak<Inner>, mut sub: nexus_kernel::EventSubscription) {
    loop {
        // Drop ref each iteration so the Arc count reflects only
        // external owners — that's what `Weak::upgrade` relies on.
        let Some(inner) = weak.upgrade() else {
            tracing::debug!("BL-007 pull-landing subscriber: publisher dropped, exiting");
            break;
        };

        // Drain pending events without blocking. We don't care how
        // many came in — one event is as good as ten for "go reload".
        let mut had_event = false;
        loop {
            match sub.try_recv() {
                Ok(Some(_)) => had_event = true,
                Ok(None) => break,
                Err(err) => {
                    tracing::debug!(%err, "BL-007 pull-landing subscriber: recv error");
                    break;
                }
            }
        }

        if had_event {
            let publisher = CrdtPublisher { inner };
            for relpath in publisher.open_relpaths() {
                match publisher.reload_after_external_change(&relpath) {
                    Ok(outcome) => {
                        if outcome.absorbed > 0 || !outcome.conflicts.is_empty() {
                            tracing::info!(
                                relpath,
                                absorbed = outcome.absorbed,
                                conflicts = outcome.conflicts.len(),
                                "BL-007 pull-landing reload"
                            );
                        }
                    }
                    Err(ReloadSkip::Missing) => {
                        // No state file for this relpath — expected
                        // for files the user hasn't edited yet.
                    }
                    Err(skip) => {
                        tracing::debug!(relpath, ?skip, "BL-007 pull-landing reload skipped");
                    }
                }
            }
        } else {
            // No event this tick — release `inner` and sleep. Without
            // this drop, the strong count would stay at 1 across the
            // sleep and we'd never see "publisher dropped".
            drop(inner);
            std::thread::sleep(PULL_LANDING_TICK);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nexus_editor::{Block, BlockTree, BlockType, DocumentMetadata, Operation};
    use nexus_kernel::{EventBus, EventFilter, NexusEvent};

    use super::*;

    fn fresh_tree() -> (BlockTree, nexus_editor::BlockId) {
        let mut tree = BlockTree::new(DocumentMetadata::default());
        let block = Block::new(BlockType::Paragraph);
        let id = block.id;
        tree.insert(block, None, 0).unwrap();
        (tree, id)
    }

    fn insert_text(block_id: nexus_editor::BlockId, pos: usize, text: &str) -> Operation {
        Operation::InsertText {
            block_id,
            pos,
            text: text.into(),
            pre_annotations: vec![],
        }
    }

    #[tokio::test]
    async fn publishes_op_envelope_on_apply_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new(64));
        let publisher = CrdtPublisher::new(dir.path().to_path_buf(), Arc::clone(&bus));

        let mut sub = bus.subscribe(EventFilter::CustomExact(ops_topic("notes.md")));

        let (tree, b) = fresh_tree();
        publisher.on_session_opened("notes.md", &tree, b"baseline");
        publisher.on_apply_transaction("notes.md", &[insert_text(b, 0, "hi")]);

        let event = tokio::time::timeout(std::time::Duration::from_secs(2), sub.recv())
            .await
            .expect("event arrived")
            .expect("non-error");
        match &event.event {
            NexusEvent::Custom { type_id, payload, .. } => {
                assert_eq!(type_id, "com.nexus.editor.ops.notes.md");
                let envelope = OpEnvelope::from_json(payload).expect("decodes");
                assert_eq!(envelope.op.id.site, publisher.site());
            }
            other => panic!("expected Custom event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn close_writes_state_and_open_restores() {
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new(64));
        let publisher = CrdtPublisher::new(dir.path().to_path_buf(), Arc::clone(&bus));

        let (tree, b) = fresh_tree();
        publisher.on_session_opened("notes.md", &tree, b"");
        publisher.on_apply_transaction("notes.md", &[insert_text(b, 0, "abc")]);
        publisher.on_session_closed("notes.md");

        // The persistence file must now exist on disk.
        let state_path = dir.path().join(crdt_state_path("notes.md"));
        assert!(state_path.exists(), "state file missing at {}", state_path.display());

        // Reopen with the *same* tree+source bytes that the editor
        // would parse from the freshly-saved markdown — the
        // publisher should restore state instead of starting fresh.
        let saved_markdown = nexus_editor::MarkdownSerializer::serialize(&{
            let mut t = BlockTree::new(DocumentMetadata::default());
            let mut b2 = Block::new(BlockType::Paragraph);
            b2.id = b;
            b2.content = "abc".into();
            t.insert(b2, None, 0).unwrap();
            t
        });
        let mut t2 = BlockTree::new(DocumentMetadata::default());
        let mut b2 = Block::new(BlockType::Paragraph);
        b2.id = b;
        b2.content = "abc".into();
        t2.insert(b2, None, 0).unwrap();

        publisher.on_session_opened("notes.md", &t2, saved_markdown.as_bytes());
        let docs = publisher.inner.docs.lock().unwrap();
        let state = docs.get("notes.md").expect("session restored");
        // Restored doc carries the prior op log.
        assert_eq!(state.doc.log().len(), 1);
    }

    #[tokio::test]
    async fn periodic_checkpoint_writes_before_close() {
        // With checkpoint_every=2, the third InsertText triggers a
        // mid-session write — so the on-disk file should exist
        // *before* on_session_closed runs.
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new(64));
        let publisher = CrdtPublisher::with_checkpoint_every(
            dir.path().to_path_buf(),
            Arc::clone(&bus),
            2,
        );

        let (tree, b) = fresh_tree();
        publisher.on_session_opened("notes.md", &tree, b"");
        let path = dir.path().join(crdt_state_path("notes.md"));
        assert!(!path.exists(), "no file before any apply");

        // First op: under threshold, no write.
        publisher.on_apply_transaction("notes.md", &[insert_text(b, 0, "a")]);
        assert!(!path.exists(), "no file after 1 op (threshold=2)");

        // Second op: hits threshold; checkpoint fires.
        publisher.on_apply_transaction("notes.md", &[insert_text(b, 1, "b")]);
        assert!(path.exists(), "checkpoint must have written by now");

        // The on-disk envelope's log length tracks the live state.
        let bytes = std::fs::read(&path).unwrap();
        let envelope: PersistedCrdt = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(envelope.state.log.len(), 2);
    }

    #[tokio::test]
    async fn undo_publishes_inverse_op_envelope() {
        // After apply + undo, the publisher should issue an envelope
        // containing a DeleteText (the inverse of the InsertText).
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new(64));
        let publisher = CrdtPublisher::with_checkpoint_every(
            dir.path().to_path_buf(),
            Arc::clone(&bus),
            0,
        );
        let mut sub = bus.subscribe(EventFilter::CustomExact(ops_topic("notes.md")));

        let (tree, b) = fresh_tree();
        publisher.on_session_opened("notes.md", &tree, b"");

        let insert = insert_text(b, 0, "hello");
        publisher.on_apply_transaction("notes.md", &[insert.clone()]);
        // Drain the apply event.
        sub.recv().await.unwrap();

        // Build the same tx the editor would have built.
        let tx = nexus_editor::Transaction::new(
            vec![insert],
            nexus_editor::TransactionMetadata::default(),
        );
        // Pretend the editor undid this tx — the publisher's mirror
        // is still post-apply, matching what the editor passes.
        let post_tree = {
            let docs = publisher.inner.docs.lock().unwrap();
            docs.get("notes.md").unwrap().doc.tree().clone()
        };
        publisher.on_undo_transaction("notes.md", &tx, &post_tree);

        let event = tokio::time::timeout(std::time::Duration::from_secs(2), sub.recv())
            .await
            .expect("undo event")
            .expect("non-error");
        if let NexusEvent::Custom { payload, .. } = &event.event {
            let envelope = OpEnvelope::from_json(payload).unwrap();
            match &envelope.op.op {
                Operation::DeleteText { deleted_text, .. } => {
                    assert_eq!(deleted_text, "hello");
                }
                other => panic!("expected DeleteText inverse, got {other:?}"),
            }
        } else {
            panic!("expected Custom event");
        }
    }

    #[tokio::test]
    async fn close_compacts_log_to_open_time_vv() {
        // BL-074 follow-up — op-log compaction wiring. Session A
        // authors ops, closes, persisting log L1. Session B reopens
        // the same file, authors more ops, closes. The post-close
        // persisted log must contain only ops authored *during*
        // session B — every op in session A's persisted state has
        // been pruned because its VV is at-or-below session B's
        // open-time VV.
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new(64));

        // Session A: 3 ops, then close.
        let publisher_a = CrdtPublisher::with_checkpoint_every(
            dir.path().to_path_buf(),
            Arc::clone(&bus),
            0,
        );
        let (tree, b) = fresh_tree();
        publisher_a.on_session_opened("notes.md", &tree, b"");
        publisher_a.on_apply_transaction("notes.md", &[insert_text(b, 0, "a")]);
        publisher_a.on_apply_transaction("notes.md", &[insert_text(b, 1, "b")]);
        publisher_a.on_apply_transaction("notes.md", &[insert_text(b, 2, "c")]);
        publisher_a.on_session_closed("notes.md");

        // Sanity: 3 ops were the only ops in the session, so nothing
        // was pruned (open-time VV was empty). Session-A log on disk
        // still has 3 entries.
        let path = dir.path().join(crdt_state_path("notes.md"));
        let bytes = std::fs::read(&path).unwrap();
        let envelope: PersistedCrdt = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(envelope.state.log.len(), 3, "session A keeps its own ops");

        // Build session B's tree from the saved markdown so the
        // content-hash check passes and the open-time restore wins.
        let saved_markdown = nexus_editor::MarkdownSerializer::serialize(&{
            let mut t = BlockTree::new(DocumentMetadata::default());
            let mut bb = Block::new(BlockType::Paragraph);
            bb.id = b;
            bb.content = "abc".into();
            t.insert(bb, None, 0).unwrap();
            t
        });
        let mut tree_b = BlockTree::new(DocumentMetadata::default());
        let mut bb = Block::new(BlockType::Paragraph);
        bb.id = b;
        bb.content = "abc".into();
        tree_b.insert(bb, None, 0).unwrap();

        // Session B: open (restoring 3 ops), author 2 more, close.
        let publisher_b = CrdtPublisher::with_checkpoint_every(
            dir.path().to_path_buf(),
            Arc::clone(&bus),
            0,
        );
        publisher_b.on_session_opened("notes.md", &tree_b, saved_markdown.as_bytes());
        publisher_b.on_apply_transaction("notes.md", &[insert_text(b, 3, "d")]);
        publisher_b.on_apply_transaction("notes.md", &[insert_text(b, 4, "e")]);
        publisher_b.on_session_closed("notes.md");

        // Post-close on disk: only the 2 session-B ops survive in the
        // log; the 3 session-A ops collapse into the prune floor.
        let bytes2 = std::fs::read(&path).unwrap();
        let envelope2: PersistedCrdt = serde_json::from_slice(&bytes2).unwrap();
        assert_eq!(envelope2.state.log.len(), 2, "session A log compacted away");
        assert!(
            !envelope2.state.log.pruned_floor().0.is_empty(),
            "prune floor advanced to cover the dropped ops"
        );

        // A peer that never saw session A's ops can still load the
        // compacted state and converge — the prune floor reports
        // those ids as already-seen.
        for op in envelope.state.log.iter() {
            assert!(
                envelope2.state.log.contains(op.id),
                "pruned op {:?} still reports as seen via the floor",
                op.id
            );
        }
    }

    #[tokio::test]
    async fn periodic_checkpoint_disabled_when_zero() {
        // checkpoint_every=0 ⇒ no mid-session writes; close is the
        // only persistence trigger.
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new(64));
        let publisher =
            CrdtPublisher::with_checkpoint_every(dir.path().to_path_buf(), Arc::clone(&bus), 0);

        let (tree, b) = fresh_tree();
        publisher.on_session_opened("notes.md", &tree, b"");
        for ch in ["a", "b", "c", "d", "e"] {
            publisher.on_apply_transaction("notes.md", &[insert_text(b, 0, ch)]);
        }
        let path = dir.path().join(crdt_state_path("notes.md"));
        assert!(!path.exists(), "no checkpoint with cadence=0");

        publisher.on_session_closed("notes.md");
        assert!(path.exists(), "close must still persist");
    }

    /// Helper for the BL-007 reload tests: write `envelope` to the
    /// publisher's state path for `relpath`, simulating what a `git
    /// pull` + merge driver would have produced on disk.
    fn write_state_file(forge_root: &std::path::Path, relpath: &str, envelope: &PersistedCrdt) {
        let path = forge_root.join(crdt_state_path(relpath));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_vec(envelope).unwrap()).unwrap();
    }

    #[tokio::test]
    async fn reload_returns_missing_when_no_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new(64));
        let publisher = CrdtPublisher::new(dir.path().to_path_buf(), Arc::clone(&bus));
        match publisher.reload_after_external_change("notes.md") {
            Err(ReloadSkip::Missing) => {}
            other => panic!("expected ReloadSkip::Missing, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn reload_returns_invalid_on_malformed_state() {
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new(64));
        let publisher = CrdtPublisher::new(dir.path().to_path_buf(), Arc::clone(&bus));
        let path = dir.path().join(crdt_state_path("notes.md"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ not valid json").unwrap();
        match publisher.reload_after_external_change("notes.md") {
            Err(ReloadSkip::Invalid) => {}
            other => panic!("expected ReloadSkip::Invalid, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn reload_returns_no_session_when_state_present_but_no_open_session() {
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new(64));
        let publisher = CrdtPublisher::new(dir.path().to_path_buf(), Arc::clone(&bus));

        // Build a valid persisted envelope from a fresh peer.
        let peer_site = SiteId::new();
        let (tree, b) = fresh_tree();
        let mut peer = CrdtDoc::new(peer_site, tree);
        peer.apply_local(&insert_text(b, 0, "remote")).unwrap();
        let envelope = PersistedCrdt::new(peer.state(), content_hash_hex(b"remote"));
        write_state_file(dir.path(), "notes.md", &envelope);

        // No session opened on the publisher.
        match publisher.reload_after_external_change("notes.md") {
            Err(ReloadSkip::NoSession) => {}
            other => panic!("expected ReloadSkip::NoSession, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn reload_absorbs_remote_ops_and_publishes_them() {
        // Live session has its own local op. The on-disk state file
        // (as if a git pull just landed via the merge driver) carries
        // both that op and a peer's op. Reload absorbs only the
        // peer's op and publishes one envelope on the ops topic.
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new(64));
        let publisher = CrdtPublisher::with_checkpoint_every(
            dir.path().to_path_buf(),
            Arc::clone(&bus),
            0,
        );
        let mut sub = bus.subscribe(EventFilter::CustomExact(ops_topic("notes.md")));

        let (tree, b) = fresh_tree();
        publisher.on_session_opened("notes.md", &tree, b"");

        // Local edit on the open session.
        publisher.on_apply_transaction("notes.md", &[insert_text(b, 0, "L")]);
        // Drain the local-apply event so the next `recv` is the remote.
        sub.recv().await.unwrap();

        // Build a "merged" log on disk: a peer site's op layered on top
        // of the same local op the open session produced. Since we
        // can't reach into the publisher's locked doc to clone it,
        // instead simulate the merge by constructing a peer doc with
        // a single new op against the same tree, and then unioning
        // the peer's log with the live session's log (read out via
        // an unrelated path: the publisher writes a checkpoint at
        // close, but here we use a direct approach — start the peer
        // from the same baseline tree, apply its op, and persist
        // *only* the peer's log. The reload path will absorb the
        // peer op without resurrecting the local one because
        // `OpLog::contains` filters duplicates by id).
        let peer_site = SiteId::new();
        let mut peer_tree = BlockTree::new(DocumentMetadata::default());
        let mut peer_block = Block::new(BlockType::Paragraph);
        peer_block.id = b;
        peer_tree.insert(peer_block, None, 0).unwrap();
        let mut peer = CrdtDoc::new(peer_site, peer_tree);
        let peer_op = peer.apply_local(&insert_text(b, 0, "R")).unwrap();
        let envelope = PersistedCrdt::new(peer.state(), content_hash_hex(b"any"));
        write_state_file(dir.path(), "notes.md", &envelope);

        let outcome = publisher
            .reload_after_external_change("notes.md")
            .expect("reload succeeded");
        assert_eq!(outcome.absorbed, 1, "only the peer op was new");
        assert!(
            outcome.conflicts.is_empty(),
            "concurrent text edits resolve via RGA, no conflict surfaces"
        );

        // The absorbed op fired on the ops topic.
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), sub.recv())
            .await
            .expect("absorbed event arrived")
            .expect("non-error");
        if let NexusEvent::Custom { payload, .. } = &event.event {
            let env = OpEnvelope::from_json(payload).unwrap();
            assert_eq!(env.op.id, peer_op.id, "envelope is the peer op");
        } else {
            panic!("expected Custom event");
        }

        // Live session's log now contains the peer op.
        {
            let docs = publisher.inner.docs.lock().unwrap();
            let state = docs.get("notes.md").unwrap();
            assert!(state.doc.log().contains(peer_op.id));
        }
    }

    #[tokio::test]
    async fn reload_publishes_conflict_envelope_on_concurrent_block_edit() {
        // Live session and a peer both run `UpdateBlockContent` on
        // the same block; peer's `vv_at_creation` doesn't include the
        // local edit. `apply_remote` flags
        // `Conflict::ConcurrentBlockEdit`; reload publishes a
        // `ConflictEnvelope` on the conflict topic.
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new(64));
        let publisher = CrdtPublisher::with_checkpoint_every(
            dir.path().to_path_buf(),
            Arc::clone(&bus),
            0,
        );
        let mut conflict_sub =
            bus.subscribe(EventFilter::CustomExact(nexus_crdt::conflict_topic("notes.md")));

        // Live tree: a paragraph block initialised to "old".
        let mut tree = BlockTree::new(DocumentMetadata::default());
        let mut block = Block::new(BlockType::Paragraph);
        block.content = "old".into();
        let b = block.id;
        tree.insert(block, None, 0).unwrap();
        publisher.on_session_opened("notes.md", &tree, b"old");

        // Local edit: replace whole-block content with "L".
        publisher.on_apply_transaction(
            "notes.md",
            &[Operation::UpdateBlockContent {
                id: b,
                old_content: "old".into(),
                new_content: "L".into(),
                old_annotations: vec![],
                new_annotations: vec![],
            }],
        );

        // Peer doc: same baseline, replaces with "R".
        let mut peer_tree = BlockTree::new(DocumentMetadata::default());
        let mut peer_block = Block::new(BlockType::Paragraph);
        peer_block.id = b;
        peer_block.content = "old".into();
        peer_tree.insert(peer_block, None, 0).unwrap();
        let mut peer = CrdtDoc::new(SiteId::new(), peer_tree);
        peer.apply_local(&Operation::UpdateBlockContent {
            id: b,
            old_content: "old".into(),
            new_content: "R".into(),
            old_annotations: vec![],
            new_annotations: vec![],
        })
        .unwrap();
        let envelope = PersistedCrdt::new(peer.state(), content_hash_hex(b"any"));
        write_state_file(dir.path(), "notes.md", &envelope);

        let outcome = publisher
            .reload_after_external_change("notes.md")
            .expect("reload succeeded");
        assert_eq!(outcome.absorbed, 0, "the conflicting op was not applied");
        assert_eq!(outcome.conflicts.len(), 1, "one conflict surfaces");
        assert!(matches!(
            outcome.conflicts[0],
            nexus_crdt::Conflict::ConcurrentBlockEdit { .. }
        ));

        // The conflict envelope fired on the conflict topic.
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), conflict_sub.recv())
            .await
            .expect("conflict event arrived")
            .expect("non-error");
        if let NexusEvent::Custom { type_id, payload, .. } = &event.event {
            assert_eq!(type_id, "com.nexus.editor.crdt.conflict.notes.md");
            let env = nexus_crdt::ConflictEnvelope::from_json(payload).unwrap();
            assert_eq!(env.conflicts.len(), 1);
        } else {
            panic!("expected Custom event");
        }
    }

    #[tokio::test]
    async fn reload_does_not_publish_conflict_envelope_when_clean() {
        // Sanity check: a clean reload (no conflicts) does NOT fire on
        // the conflict topic. The empty-list short-circuit lives in
        // `publish_conflicts`.
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new(64));
        let publisher = CrdtPublisher::with_checkpoint_every(
            dir.path().to_path_buf(),
            Arc::clone(&bus),
            0,
        );
        let mut conflict_sub =
            bus.subscribe(EventFilter::CustomExact(nexus_crdt::conflict_topic("notes.md")));

        let (tree, b) = fresh_tree();
        publisher.on_session_opened("notes.md", &tree, b"");

        // Peer adds an op to the same block, no concurrent local edit
        // — RGA absorbs silently, no conflict.
        let mut peer_tree = BlockTree::new(DocumentMetadata::default());
        let mut peer_block = Block::new(BlockType::Paragraph);
        peer_block.id = b;
        peer_tree.insert(peer_block, None, 0).unwrap();
        let mut peer = CrdtDoc::new(SiteId::new(), peer_tree);
        peer.apply_local(&insert_text(b, 0, "R")).unwrap();
        let envelope = PersistedCrdt::new(peer.state(), content_hash_hex(b"any"));
        write_state_file(dir.path(), "notes.md", &envelope);

        let outcome = publisher
            .reload_after_external_change("notes.md")
            .expect("reload succeeded");
        assert_eq!(outcome.absorbed, 1);
        assert!(outcome.conflicts.is_empty());

        // Conflict topic stays silent.
        match conflict_sub.try_recv() {
            Ok(None) | Err(_) => {}
            Ok(Some(_)) => panic!("conflict topic should not fire on clean reload"),
        }
    }

    #[tokio::test]
    async fn pull_landing_subscriber_reloads_on_git_commit_event() {
        // End-to-end: subscriber thread + bus + publisher. After a
        // `com.nexus.git.commit` event fires, the thread re-reads the
        // state file and absorbs any new ops, republishing them on
        // the ops topic.
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new(64));
        let publisher = CrdtPublisher::with_checkpoint_every(
            dir.path().to_path_buf(),
            Arc::clone(&bus),
            0,
        );
        let mut ops_sub = bus.subscribe(EventFilter::CustomExact(ops_topic("notes.md")));

        let (tree, b) = fresh_tree();
        publisher.on_session_opened("notes.md", &tree, b"");

        // Spawn the subscriber. Start it *after* the session is open
        // so the first reload the thread runs has something to find.
        let _handle = publisher.start_pull_landing_subscriber();

        // Build a peer's state and drop it on disk where the merge
        // driver would have left it after a pull.
        let peer_site = SiteId::new();
        let mut peer_tree = BlockTree::new(DocumentMetadata::default());
        let mut peer_block = Block::new(BlockType::Paragraph);
        peer_block.id = b;
        peer_tree.insert(peer_block, None, 0).unwrap();
        let mut peer = CrdtDoc::new(peer_site, peer_tree);
        let peer_op = peer.apply_local(&insert_text(b, 0, "R")).unwrap();
        let envelope = PersistedCrdt::new(peer.state(), content_hash_hex(b"any"));
        write_state_file(dir.path(), "notes.md", &envelope);

        // Fire the event the subscriber listens for.
        bus.publish_plugin(
            "com.nexus.git",
            "com.nexus.git.commit",
            serde_json::json!({"head": "deadbeef"}),
        )
        .unwrap();

        // The subscriber polls every 250ms; expect the absorbed
        // envelope on the ops topic within a couple of ticks.
        let event = tokio::time::timeout(std::time::Duration::from_secs(3), ops_sub.recv())
            .await
            .expect("absorbed op envelope arrived within 3s")
            .expect("non-error");
        if let NexusEvent::Custom { payload, .. } = &event.event {
            let env = OpEnvelope::from_json(payload).unwrap();
            assert_eq!(env.op.id, peer_op.id, "envelope is the peer op");
        } else {
            panic!("expected Custom event");
        }
    }

    #[test]
    fn pull_landing_subscriber_exits_when_publisher_drops() {
        // Strong-ref release: dropping the last `CrdtPublisher` clone
        // causes the thread's `Weak::upgrade` to fail and exit. No
        // explicit shutdown signal needed — that's the whole reason
        // we use `Weak` instead of an `AtomicBool`.
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new(64));
        let publisher = CrdtPublisher::new(dir.path().to_path_buf(), Arc::clone(&bus));
        let handle = publisher.start_pull_landing_subscriber();
        drop(publisher);
        // Worst case is one tick of sleep (250ms) plus the time to
        // notice the upgrade failure. Allow generous slack so a busy
        // CI host doesn't flake.
        let join_result = std::thread::Builder::new()
            .name("test-join-watchdog".to_string())
            .spawn(move || handle.join())
            .unwrap()
            .join();
        match join_result {
            Ok(Ok(())) => {}
            Ok(Err(_)) => panic!("subscriber thread panicked"),
            Err(_) => panic!("watchdog thread panicked"),
        }
    }

    #[tokio::test]
    async fn reload_is_idempotent_when_state_already_absorbed() {
        // Calling reload twice with the same state file is a no-op
        // the second time — `apply_remote` filters duplicates by id.
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new(64));
        let publisher = CrdtPublisher::with_checkpoint_every(
            dir.path().to_path_buf(),
            Arc::clone(&bus),
            0,
        );

        let (tree, b) = fresh_tree();
        publisher.on_session_opened("notes.md", &tree, b"");

        let peer_site = SiteId::new();
        let mut peer_tree = BlockTree::new(DocumentMetadata::default());
        let mut peer_block = Block::new(BlockType::Paragraph);
        peer_block.id = b;
        peer_tree.insert(peer_block, None, 0).unwrap();
        let mut peer = CrdtDoc::new(peer_site, peer_tree);
        peer.apply_local(&insert_text(b, 0, "R")).unwrap();
        let envelope = PersistedCrdt::new(peer.state(), content_hash_hex(b"any"));
        write_state_file(dir.path(), "notes.md", &envelope);

        let first = publisher
            .reload_after_external_change("notes.md")
            .expect("first reload");
        assert_eq!(first.absorbed, 1);

        let second = publisher
            .reload_after_external_change("notes.md")
            .expect("second reload");
        assert_eq!(second.absorbed, 0, "no new ops on the second pass");
    }
}
