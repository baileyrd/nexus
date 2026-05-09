//! Phase 3 sync loop: pumps remote [`CrdtOp`]s off the kernel event
//! bus into a [`CrdtDoc`].
//!
//! The editor core plugin authors local ops via [`CrdtDoc::apply_local`]
//! and publishes the resulting wire op to the per-file topic
//! `com.nexus.editor.ops.<relpath>` (see [`crate::wire`]). Other peers
//! — popout windows (ADR 0020), CLI / TUI sessions on the same forge,
//! or future cross-process collaborators — run a [`SyncLoop`] which
//! subscribes to that topic and feeds inbound ops into their own
//! [`CrdtDoc`].
//!
//! The loop drops ops that originated on the local site (`op.id.site
//! == doc.site()`) so a single bus shared by author + receiver in the
//! same process doesn't cause infinite echoes.
//!
//! # Lifecycle
//!
//! - [`SyncLoop::new`] takes ownership of a [`CrdtDoc`] (wrapped in a
//!   shared mutex so multiple readers can observe state) and an
//!   [`EventSubscription`] already filtered to the per-file topic.
//! - [`SyncLoop::run`] consumes the subscription and applies ops until
//!   the bus closes or the caller drops the future. Errors that don't
//!   warrant termination (decode failures on a single payload,
//!   conflicts the doc surfaces) are logged and skipped.
//! - [`SyncLoop::doc`] hands out a clone of the shared doc handle so
//!   external code can read the current tree (or apply locally).

use std::sync::{Arc, Mutex};

use nexus_kernel::{EventFilter, EventSubscription, NexusEvent};
use serde::{Deserialize, Serialize};

use crate::doc::{CrdtDoc, RemoteOutcome};
use crate::error::{CrdtError, Result};
use crate::op::CrdtOp;
use crate::wire::{ops_topic, OpEnvelope};

/// Outcome of applying a single inbound op via [`SyncLoop::apply_remote_payload`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum InboundOutcome {
    /// Applied cleanly (or already applied — duplicates are idempotent).
    Applied,
    /// The op originated on this site; the loop dropped it without
    /// touching the doc. Prevents echo loops on a shared bus.
    SelfEcho,
    /// The op surfaced a conflict. Caller may want to forward to UI.
    Conflict(crate::Conflict),
}

/// A handle that owns a [`CrdtDoc`] behind an [`Arc<Mutex<…>>`] so the
/// run loop and external callers can both reach it. Cloneable.
#[derive(Clone)]
pub struct DocHandle {
    inner: Arc<Mutex<CrdtDoc>>,
}

impl DocHandle {
    /// Wrap an existing doc.
    #[must_use]
    pub fn new(doc: CrdtDoc) -> Self {
        Self {
            inner: Arc::new(Mutex::new(doc)),
        }
    }

    /// Read-locked access. Returns an error if the lock is poisoned.
    ///
    /// # Errors
    ///
    /// Returns [`CrdtError::Wire`] with a `"doc lock poisoned"` message
    /// if a previous holder of the lock panicked while holding it.
    pub fn with_doc<R>(&self, f: impl FnOnce(&CrdtDoc) -> R) -> Result<R> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| CrdtError::Wire("doc lock poisoned".into()))?;
        Ok(f(&guard))
    }

    /// Mutable-locked access. Returns an error if the lock is poisoned.
    ///
    /// # Errors
    ///
    /// Returns [`CrdtError::Wire`] with a `"doc lock poisoned"` message
    /// if a previous holder of the lock panicked while holding it.
    pub fn with_doc_mut<R>(&self, f: impl FnOnce(&mut CrdtDoc) -> R) -> Result<R> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| CrdtError::Wire("doc lock poisoned".into()))?;
        Ok(f(&mut guard))
    }
}

/// Sync loop: pumps remote ops off an [`EventSubscription`] into a
/// [`CrdtDoc`].
pub struct SyncLoop {
    doc: DocHandle,
    sub: EventSubscription,
    /// Topic this loop is filtered to (informational — the
    /// `EventSubscription` already enforces it, but exposing it lets
    /// the editor log which file it's tracking).
    topic: String,
}

impl SyncLoop {
    /// Build a sync loop around `doc` that subscribes to ops for the
    /// given forge-relative path on `bus`. The subscription is created
    /// from the bus immediately so the caller can drop the bus handle
    /// before [`Self::run`] is awaited.
    #[must_use]
    pub fn new(doc: DocHandle, bus: &nexus_kernel::EventBus, relpath: &str) -> Self {
        let topic = ops_topic(relpath);
        let sub = bus.subscribe(EventFilter::CustomExact(topic.clone()));
        Self { doc, sub, topic }
    }

    /// Build a sync loop with a caller-provided subscription. Used by
    /// tests and by integrations that want to share a single
    /// subscription across multiple loops.
    #[must_use]
    pub fn from_parts(doc: DocHandle, sub: EventSubscription, topic: String) -> Self {
        Self { doc, sub, topic }
    }

    /// Topic this loop is listening on.
    #[must_use]
    pub fn topic(&self) -> &str {
        &self.topic
    }

    /// Doc handle (cloneable).
    #[must_use]
    pub fn doc(&self) -> DocHandle {
        self.doc.clone()
    }

    /// Apply one wire payload to the doc. Decodes the [`OpEnvelope`],
    /// drops self-echoes, and dispatches to [`CrdtDoc::apply_remote`].
    ///
    /// # Errors
    ///
    /// - [`CrdtError::Wire`] if the payload cannot be decoded.
    /// - [`CrdtError::Editor`] if the op cannot apply.
    pub fn apply_remote_payload(&self, payload: &serde_json::Value) -> Result<InboundOutcome> {
        let envelope = OpEnvelope::from_json(payload)?;
        self.apply_remote_op(envelope.op)
    }

    /// Apply one already-decoded op. Drops self-echoes; dispatches to
    /// [`CrdtDoc::apply_remote`] otherwise.
    ///
    /// # Errors
    ///
    /// - [`CrdtError::Editor`] if the op cannot apply.
    /// - [`CrdtError::Wire`] if the doc lock is poisoned.
    pub fn apply_remote_op(&self, op: CrdtOp) -> Result<InboundOutcome> {
        let local_site = self.doc.with_doc(super::doc::CrdtDoc::site)?;
        if op.id.site == local_site {
            return Ok(InboundOutcome::SelfEcho);
        }
        let outcome = self.doc.with_doc_mut(|d| d.apply_remote(op))??;
        Ok(match outcome {
            RemoteOutcome::Applied | RemoteOutcome::Duplicate => InboundOutcome::Applied,
            RemoteOutcome::Conflict(c) => InboundOutcome::Conflict(c),
        })
    }

    /// Run the loop until the bus closes or the future is dropped. Bus
    /// `Lagged` errors recover automatically (the loop logs the gap
    /// and continues). Decode failures on a single payload don't
    /// terminate the loop — they're logged and the loop moves on.
    pub async fn run(mut self) {
        tracing::debug!(topic = %self.topic, "crdt sync loop started");
        loop {
            match self.sub.recv().await {
                Ok(event) => {
                    if let NexusEvent::Custom { payload, .. } = &event.event {
                        match self.apply_remote_payload(payload) {
                            Ok(InboundOutcome::Applied | InboundOutcome::SelfEcho) => {}
                            Ok(InboundOutcome::Conflict(c)) => {
                                tracing::warn!(
                                    topic = %self.topic,
                                    block = %c.block_id(),
                                    "crdt remote op surfaced conflict",
                                );
                            }
                            Err(err) => {
                                tracing::warn!(
                                    topic = %self.topic,
                                    %err,
                                    "crdt remote op rejected",
                                );
                            }
                        }
                    }
                }
                Err(nexus_kernel::RecvError::Lagged(n)) => {
                    tracing::warn!(
                        topic = %self.topic,
                        skipped = n,
                        "crdt sync loop lagged — events dropped",
                    );
                }
                Err(nexus_kernel::RecvError::Closed) => {
                    tracing::debug!(topic = %self.topic, "crdt sync loop bus closed");
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nexus_editor::{Block, BlockTree, BlockType, DocumentMetadata, Operation};
    use nexus_kernel::EventBus;

    use super::*;
    use crate::id::SiteId;
    use crate::wire::OpEnvelope;

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
    async fn applies_remote_op_through_envelope() {
        let bus = Arc::new(EventBus::new(64));
        let (tree, b) = fresh_tree();

        let local = SiteId::new();
        let remote = SiteId::new();

        let local_doc = CrdtDoc::new(local, tree.clone());
        let local_handle = DocHandle::new(local_doc);
        let sync = SyncLoop::new(local_handle.clone(), &bus, "notes.md");

        // Author an op on the remote site and gossip it.
        let mut remote_doc = CrdtDoc::new(remote, tree);
        let wire = remote_doc.apply_local(&insert_text(b, 0, "hello")).unwrap();
        let payload = OpEnvelope::new(wire).to_json().unwrap();

        let outcome = sync.apply_remote_payload(&payload).unwrap();
        assert!(matches!(outcome, InboundOutcome::Applied));
        local_handle
            .with_doc(|d| {
                assert_eq!(d.tree().get(b).unwrap().content, "hello");
            })
            .unwrap();
    }

    #[tokio::test]
    async fn drops_self_echo() {
        let bus = Arc::new(EventBus::new(64));
        let (tree, b) = fresh_tree();
        let site = SiteId::new();
        let mut doc = CrdtDoc::new(site, tree);
        // Author a local op — its id.site will equal `site`.
        let wire = doc.apply_local(&insert_text(b, 0, "x")).unwrap();
        let payload = OpEnvelope::new(wire).to_json().unwrap();

        let handle = DocHandle::new(doc);
        let sync = SyncLoop::new(handle, &bus, "notes.md");
        let outcome = sync.apply_remote_payload(&payload).unwrap();
        assert!(matches!(outcome, InboundOutcome::SelfEcho));
    }

    #[tokio::test]
    async fn run_loop_drains_bus_and_converges() {
        // End-to-end: two docs sharing a single bus. Site A authors
        // locally and publishes; site B's SyncLoop drains and applies.
        let bus = Arc::new(EventBus::new(64));
        let (tree, b) = fresh_tree();

        let site_a = SiteId::new();
        let site_b = SiteId::new();
        let mut doc_a = CrdtDoc::new(site_a, tree.clone());
        let doc_b = CrdtDoc::new(site_b, tree);
        let handle_b = DocHandle::new(doc_b);

        let topic = ops_topic("notes.md");
        let sync_b = SyncLoop::new(handle_b.clone(), &bus, "notes.md");
        let task = tokio::spawn(sync_b.run());

        // Site A authors an op and publishes via the bus.
        let wire = doc_a.apply_local(&insert_text(b, 0, "hello")).unwrap();
        let payload = OpEnvelope::new(wire).to_json().unwrap();
        bus.publish_plugin("com.nexus.editor", &topic, payload)
            .unwrap();

        // Yield until B picks it up. With a fresh bus + single
        // subscriber the publish lands immediately, but `recv()` is
        // async so we need at least one yield to let it run.
        for _ in 0..32 {
            tokio::task::yield_now().await;
            let content = handle_b
                .with_doc(|d| d.tree().get(b).unwrap().content.clone())
                .unwrap();
            if content == "hello" {
                break;
            }
        }

        // Drop the bus to close the broadcast channel — this lets the
        // run loop exit cleanly so the test doesn't hang.
        drop(bus);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), task).await;

        let final_content = handle_b
            .with_doc(|d| d.tree().get(b).unwrap().content.clone())
            .unwrap();
        assert_eq!(final_content, "hello");
    }
}
