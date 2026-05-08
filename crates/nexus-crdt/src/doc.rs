//! [`CrdtDoc`]: a [`BlockTree`] paired with the CRDT op log.
//!
//! `CrdtDoc` is the doc-level façade the editor and sync loops drive:
//!
//! - `apply_local(op)` — assigns a fresh [`OpId`], applies the editor
//!   op to the tree, and appends the resulting [`CrdtOp`] to the log.
//!   Returns the wire op for the sync layer to gossip.
//! - `apply_remote(op)` — checks idempotency, runs the conflict
//!   detector, and either applies cleanly or surfaces a [`Conflict`].
//!
//! Phase 1 is **conservative**: any concurrent edit that touches the
//! same block surfaces as a [`Conflict`]. Phase 2 adds silent merge
//! for character-level concurrent text edits via [`crate::text`].

use std::collections::HashMap;

use nexus_editor::{BlockId, BlockTree, Operation};
use serde::{Deserialize, Serialize};

use crate::conflict::Conflict;
use crate::error::Result;
use crate::id::{Lamport, OpId, SiteId};
use crate::log::OpLog;
use crate::op::{affected_blocks, primary_block_id, CrdtOp};

/// Per-block annotation tracking which op last touched it. This is the
/// minimal state needed to detect "two sites wrote here without seeing
/// each other".
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct BlockMeta {
    /// Op id of the last *applied* op that mutated this block.
    last_writer: Option<OpId>,
    /// Whether the block has been deleted by a tombstone op.
    deleted_by: Option<OpId>,
}

/// CRDT-aware document: the [`BlockTree`] plus its op log and conflict
/// state.
#[derive(Clone, Debug)]
pub struct CrdtDoc {
    site: SiteId,
    lamport: Lamport,
    tree: BlockTree,
    log: OpLog,
    block_meta: HashMap<BlockId, BlockMeta>,
}

/// Outcome of `apply_remote`.
#[derive(Clone, Debug)]
pub enum RemoteOutcome {
    /// The op was already applied (duplicate). No state change.
    Duplicate,
    /// The op was applied cleanly.
    Applied,
    /// The op was rejected by conflict detection. The caller must
    /// surface this to the user / resolution UI.
    Conflict(Conflict),
}

impl CrdtDoc {
    /// Wrap an existing [`BlockTree`] with a fresh op log under
    /// `site`'s authority. The starting lamport is zero — this is the
    /// "initial document" baseline.
    #[must_use]
    pub fn new(site: SiteId, tree: BlockTree) -> Self {
        let mut block_meta = HashMap::new();
        for &block_id in tree.blocks.keys() {
            block_meta.insert(block_id, BlockMeta::default());
        }
        Self {
            site,
            lamport: Lamport::default(),
            tree,
            log: OpLog::new(),
            block_meta,
        }
    }

    /// Borrow the underlying tree.
    #[must_use]
    pub fn tree(&self) -> &BlockTree {
        &self.tree
    }

    /// Borrow the op log.
    #[must_use]
    pub fn log(&self) -> &OpLog {
        &self.log
    }

    /// This site's id.
    #[must_use]
    pub fn site(&self) -> SiteId {
        self.site
    }

    /// Apply a locally-authored editor op. Returns the wire-format
    /// [`CrdtOp`] to gossip to peers.
    ///
    /// # Errors
    ///
    /// Propagates [`CrdtError::Editor`] if the op cannot apply (e.g.
    /// it references a missing block).
    pub fn apply_local(&mut self, op: &Operation) -> Result<CrdtOp> {
        self.lamport = self.lamport.next();
        let id = OpId::new(self.site, self.lamport);
        op.apply(&mut self.tree)?;
        self.update_block_meta(op, id);

        let crdt = CrdtOp {
            id,
            vv_at_creation: self.log.version_vector().clone(),
            op: op.clone(),
        };
        let appended = self.log.append(crdt.clone());
        debug_assert!(appended, "fresh local op id cannot collide");
        Ok(crdt)
    }

    /// Apply a remote op. Idempotent.
    ///
    /// # Errors
    ///
    /// Returns [`CrdtError::Editor`] only when the underlying tree
    /// rejects the op for reasons other than the conflict cases the
    /// CRDT detects (e.g. malformed wire data). Concurrency conflicts
    /// are returned via [`RemoteOutcome::Conflict`], not as errors.
    pub fn apply_remote(&mut self, op: CrdtOp) -> Result<RemoteOutcome> {
        if self.log.contains(op.id) {
            return Ok(RemoteOutcome::Duplicate);
        }
        // Track the highest lamport observed so future locally-authored
        // ops dominate it.
        if op.id.lamport > self.lamport {
            self.lamport = op.id.lamport;
        }

        if let Some(conflict) = self.detect_conflict(&op) {
            return Ok(RemoteOutcome::Conflict(conflict));
        }

        op.op.apply(&mut self.tree)?;
        self.update_block_meta(&op.op, op.id);
        let appended = self.log.append(op);
        debug_assert!(appended, "non-duplicate remote op must append");
        Ok(RemoteOutcome::Applied)
    }

    /// Look at every block this op affects and decide whether the
    /// remote and local edits saw each other. Two ops conflict iff
    /// neither's `vv_at_creation` covers the other's id.
    ///
    /// Returns `None` if safe; otherwise the most-blocking conflict
    /// (structural delete-edit dominates concurrent edit).
    fn detect_conflict(&self, remote: &CrdtOp) -> Option<Conflict> {
        let blocks = affected_blocks(&remote.op);
        let primary = primary_block_id(&remote.op);

        // Structural conflict (delete + edit) takes precedence — surface
        // even if a different block also has a concurrent edit.
        for block in &blocks {
            if let Some(meta) = self.block_meta.get(block) {
                if let Some(delete_id) = meta.deleted_by {
                    if !remote.vv_at_creation.contains(delete_id) {
                        return Some(Conflict::StructuralDeleteEdit {
                            block_id: *block,
                            delete: delete_id,
                            edit: remote.id,
                        });
                    }
                }
                // Symmetric: this op is itself a delete and the local
                // last writer didn't see it.
                if matches!(remote.op, Operation::DeleteBlock { .. }) {
                    if let Some(last) = meta.last_writer {
                        let saw_local = remote.vv_at_creation.contains(last);
                        if !saw_local && !self.log_op_saw(last, remote.id) {
                            return Some(Conflict::StructuralDeleteEdit {
                                block_id: *block,
                                delete: remote.id,
                                edit: last,
                            });
                        }
                    }
                }
            }
        }

        // Plain concurrent same-block content edit. Only flag when the
        // remote op is content-bearing on the primary block.
        if !is_content_op(&remote.op) {
            return None;
        }
        let meta = self.block_meta.get(&primary)?;
        let local_last = meta.last_writer?;
        if remote.vv_at_creation.contains(local_last) {
            return None;
        }
        Some(Conflict::ConcurrentBlockEdit {
            block_id: primary,
            local: local_last,
            remote: remote.id,
        })
    }

    fn log_op_saw(&self, candidate: OpId, observer: OpId) -> bool {
        // Did the op with id `observer` (already applied locally or
        // about to be applied as remote) causally include `candidate`?
        // For local ops we trust their creation VV; for remote we
        // trust theirs. Both paths route through `vv_at_creation`.
        self.log
            .get(observer)
            .is_some_and(|o| o.vv_at_creation.contains(candidate))
    }

    fn update_block_meta(&mut self, op: &Operation, id: OpId) {
        for block in affected_blocks(op) {
            let meta = self.block_meta.entry(block).or_default();
            meta.last_writer = Some(id);
        }
        match op {
            Operation::DeleteBlock { old_block, .. } => {
                let meta = self.block_meta.entry(old_block.id).or_default();
                meta.deleted_by = Some(id);
            }
            Operation::InsertBlock { block, .. } => {
                self.block_meta.entry(block.id).or_default();
            }
            _ => {}
        }
    }
}

/// True for ops that mutate the *content* of an existing block, as
/// opposed to its placement in the tree. Conflict detection only
/// flags concurrent content edits — concurrent reparents on the same
/// block are deliberately out of Phase 1 scope (see ADR 0026 §4).
fn is_content_op(op: &Operation) -> bool {
    matches!(
        op,
        Operation::InsertText { .. }
            | Operation::DeleteText { .. }
            | Operation::UpdateBlockContent { .. }
            | Operation::UpdateAnnotations { .. }
    )
}

#[cfg(test)]
mod tests {
    use nexus_editor::{Block, BlockTree, BlockType, Operation};

    use super::*;
    use crate::id::SiteId;

    use nexus_editor::DocumentMetadata;

    fn empty_tree() -> BlockTree {
        BlockTree::new(DocumentMetadata::default())
    }

    fn empty_tree_with_block() -> (BlockTree, BlockId) {
        let mut tree = empty_tree();
        let block = Block::new(BlockType::Paragraph);
        let id = block.id;
        tree.insert(block, None, 0).unwrap();
        (tree, id)
    }

    fn insert_text_op(block_id: BlockId, pos: usize, text: &str) -> Operation {
        Operation::InsertText {
            block_id,
            pos,
            text: text.into(),
            pre_annotations: vec![],
        }
    }

    #[test]
    fn local_apply_appends_to_log_and_mutates_tree() {
        let s = SiteId::new();
        let (tree, b) = empty_tree_with_block();
        let mut doc = CrdtDoc::new(s, tree);

        let wire = doc.apply_local(&insert_text_op(b, 0, "hello")).unwrap();
        assert_eq!(doc.log().len(), 1);
        assert_eq!(wire.id.site, s);
        assert_eq!(wire.id.lamport, Lamport(1));
        assert_eq!(doc.tree().get(b).unwrap().content, "hello");
    }

    #[test]
    fn remote_duplicate_is_no_op() {
        let s_local = SiteId::new();
        let s_remote = SiteId::new();
        let (tree, b) = empty_tree_with_block();
        let mut doc = CrdtDoc::new(s_local, tree);

        let remote_op = CrdtOp {
            id: OpId::new(s_remote, Lamport(7)),
            vv_at_creation: doc.log().version_vector().clone(),
            op: insert_text_op(b, 0, "hi"),
        };
        let first = doc.apply_remote(remote_op.clone()).unwrap();
        assert!(matches!(first, RemoteOutcome::Applied));
        let dup = doc.apply_remote(remote_op).unwrap();
        assert!(matches!(dup, RemoteOutcome::Duplicate));
        assert_eq!(doc.log().len(), 1);
        assert_eq!(doc.tree().get(b).unwrap().content, "hi");
    }

    #[test]
    fn concurrent_edits_to_different_blocks_converge() {
        // Two sites starting from the same baseline (one block A, one
        // block B). Each site edits "its" block independently — they
        // gossip and both should converge to the same tree.
        let s1 = SiteId::new();
        let s2 = SiteId::new();

        let mut tree = empty_tree();
        let block_a = Block::new(BlockType::Paragraph);
        let block_b = Block::new(BlockType::Paragraph);
        let a = block_a.id;
        let b = block_b.id;
        tree.insert(block_a, None, 0).unwrap();
        tree.insert(block_b, None, 1).unwrap();

        let mut doc1 = CrdtDoc::new(s1, tree.clone());
        let mut doc2 = CrdtDoc::new(s2, tree);

        let op1 = doc1.apply_local(&insert_text_op(a, 0, "from-1")).unwrap();
        let op2 = doc2.apply_local(&insert_text_op(b, 0, "from-2")).unwrap();

        // Cross-apply.
        assert!(matches!(
            doc1.apply_remote(op2).unwrap(),
            RemoteOutcome::Applied
        ));
        assert!(matches!(
            doc2.apply_remote(op1).unwrap(),
            RemoteOutcome::Applied
        ));

        assert_eq!(doc1.tree().get(a).unwrap().content, "from-1");
        assert_eq!(doc1.tree().get(b).unwrap().content, "from-2");
        assert_eq!(doc2.tree().get(a).unwrap().content, "from-1");
        assert_eq!(doc2.tree().get(b).unwrap().content, "from-2");
    }

    #[test]
    fn concurrent_edits_to_same_block_surface_as_conflict() {
        let s1 = SiteId::new();
        let s2 = SiteId::new();
        let (tree, b) = empty_tree_with_block();
        let mut doc1 = CrdtDoc::new(s1, tree.clone());
        let mut doc2 = CrdtDoc::new(s2, tree);

        let op1 = doc1.apply_local(&insert_text_op(b, 0, "alpha")).unwrap();
        let op2 = doc2.apply_local(&insert_text_op(b, 0, "beta")).unwrap();

        // doc1 receives doc2's op — concurrent (neither saw the other).
        match doc1.apply_remote(op2).unwrap() {
            RemoteOutcome::Conflict(Conflict::ConcurrentBlockEdit {
                block_id, local, remote,
            }) => {
                assert_eq!(block_id, b);
                assert_eq!(local, op1.id);
                assert_eq!(remote.site, s2);
            }
            other => panic!("expected ConcurrentBlockEdit, got {other:?}"),
        }
    }

    #[test]
    fn sequential_edits_do_not_conflict_after_gossip() {
        // Site 1 edits, gossips to site 2; site 2 edits afterwards
        // (causally aware of site 1's op). Site 1 must accept site 2's
        // follow-up cleanly — no conflict.
        let s1 = SiteId::new();
        let s2 = SiteId::new();
        let (tree, b) = empty_tree_with_block();
        let mut doc1 = CrdtDoc::new(s1, tree.clone());
        let mut doc2 = CrdtDoc::new(s2, tree);

        let op1 = doc1.apply_local(&insert_text_op(b, 0, "AA")).unwrap();
        assert!(matches!(
            doc2.apply_remote(op1.clone()).unwrap(),
            RemoteOutcome::Applied
        ));
        let op2 = doc2.apply_local(&insert_text_op(b, 2, "BB")).unwrap();

        // Site 2's op was authored *after* seeing site 1's — no conflict.
        match doc1.apply_remote(op2).unwrap() {
            RemoteOutcome::Applied => {}
            other => panic!("expected Applied, got {other:?}"),
        }
        assert_eq!(doc1.tree().get(b).unwrap().content, "AABB");
        assert_eq!(doc2.tree().get(b).unwrap().content, "AABB");
    }

    #[test]
    fn structural_delete_edit_surfaces() {
        let s1 = SiteId::new();
        let s2 = SiteId::new();
        let (tree, b) = empty_tree_with_block();
        let block_obj = tree.get(b).unwrap().clone();
        let mut doc1 = CrdtDoc::new(s1, tree.clone());
        let mut doc2 = CrdtDoc::new(s2, tree);

        // Site 1 deletes the block.
        let delete_op = doc1
            .apply_local(&Operation::DeleteBlock {
                old_block: block_obj,
                was_parent_id: None,
                was_index_in_parent: 0,
            })
            .unwrap();
        // Site 2, unaware, edits its content.
        let edit_op = doc2.apply_local(&insert_text_op(b, 0, "still here")).unwrap();

        // Site 1 receives the concurrent edit on a now-deleted block.
        match doc1.apply_remote(edit_op).unwrap() {
            RemoteOutcome::Conflict(Conflict::StructuralDeleteEdit {
                block_id, delete, edit,
            }) => {
                assert_eq!(block_id, b);
                assert_eq!(delete, delete_op.id);
                assert_eq!(edit.site, s2);
            }
            other => panic!("expected StructuralDeleteEdit, got {other:?}"),
        }
    }
}
