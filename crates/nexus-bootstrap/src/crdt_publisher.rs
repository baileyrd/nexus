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
use std::sync::{Arc, Mutex};

use nexus_crdt::{
    content_hash_hex, crdt_state_path, ops_topic, CrdtDoc, OpEnvelope, PersistedCrdt, SiteId,
};
use nexus_editor::{BlockTree, OpObserver, Operation, Transaction, EDITOR_PLUGIN_ID};
use nexus_kernel::EventBus;
use serde_json::Value;

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
        guard.insert(
            relpath.to_string(),
            SessionState {
                doc,
                ops_since_checkpoint: 0,
            },
        );
    }

    fn on_session_closed(&self, relpath: &str) {
        let mut guard = match self.inner.docs.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some(state) = guard.remove(relpath) {
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
}
