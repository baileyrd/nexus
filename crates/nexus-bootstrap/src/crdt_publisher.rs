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
use nexus_editor::{BlockTree, OpObserver, Operation, EDITOR_PLUGIN_ID};
use nexus_kernel::EventBus;
use serde_json::Value;

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
    docs: Mutex<HashMap<String, CrdtDoc>>,
}

impl CrdtPublisher {
    /// Construct a publisher rooted at `forge_root`, publishing on
    /// `bus`. Generates a fresh per-process [`SiteId`].
    #[must_use]
    pub fn new(forge_root: PathBuf, bus: Arc<EventBus>) -> Self {
        Self {
            inner: Arc::new(Inner {
                forge_root,
                site: SiteId::new(),
                bus,
                docs: Mutex::new(HashMap::new()),
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
        let path = self.state_file(relpath);
        if let Some(parent) = path.parent() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                tracing::warn!(%err, path = %parent.display(), "BL-074: mkdir failed");
                return;
            }
        }
        let envelope = PersistedCrdt::new(doc.state(), content_hash_hex(source_bytes));
        let bytes = match serde_json::to_vec(&envelope) {
            Ok(b) => b,
            Err(err) => {
                tracing::warn!(%err, "BL-074: encode persisted crdt failed");
                return;
            }
        };
        // Atomic write: temp + rename on the same filesystem so a
        // crash mid-write can't leave a half-written file.
        let tmp = path.with_extension("json.tmp");
        if let Err(err) = std::fs::write(&tmp, &bytes) {
            tracing::warn!(%err, path = %tmp.display(), "BL-074: tmp write failed");
            return;
        }
        if let Err(err) = std::fs::rename(&tmp, &path) {
            tracing::warn!(%err, path = %path.display(), "BL-074: rename failed");
            let _ = std::fs::remove_file(&tmp);
        }
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
        guard.insert(relpath.to_string(), doc);
    }

    fn on_session_closed(&self, relpath: &str) {
        let mut guard = match self.inner.docs.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some(doc) = guard.remove(relpath) {
            let source_bytes = Self::canonical_markdown_bytes(&doc);
            self.write_persisted(relpath, &doc, &source_bytes);
        }
    }

    fn on_apply_transaction(&self, relpath: &str, ops: &[Operation]) {
        // Author the wire ops under the lock, then publish outside it
        // so a slow subscriber can't stall future apply_transactions.
        let wire_ops: Vec<_> = {
            let mut guard = match self.inner.docs.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            let Some(doc) = guard.get_mut(relpath) else {
                tracing::debug!(
                    relpath,
                    "BL-074: apply_transaction with no observed session — dropped"
                );
                return;
            };
            ops.iter()
                .filter_map(|op| match doc.apply_local(op) {
                    Ok(wire) => Some(wire),
                    Err(err) => {
                        tracing::warn!(%err, relpath, "BL-074: apply_local rejected op");
                        None
                    }
                })
                .collect()
        };
        for wire in wire_ops {
            self.publish_op(relpath, wire);
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
        let doc = docs.get("notes.md").expect("session restored");
        // Restored doc carries the prior op log.
        assert_eq!(doc.log().len(), 1);
    }
}
