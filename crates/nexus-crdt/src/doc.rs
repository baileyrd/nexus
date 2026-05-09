//! [`CrdtDoc`]: a [`BlockTree`] paired with the CRDT op log and a
//! per-block [`RgaText`] mirror.
//!
//! `CrdtDoc` is the doc-level façade the editor and sync loops drive:
//!
//! - `apply_local(op)` — assigns a fresh [`OpId`], translates the editor
//!   op to position-free [`crate::text::RgaTextOp`]s against the
//!   current RGA, applies the editor op to the tree, mirrors the RGA,
//!   and appends the resulting [`CrdtOp`] to the log. Returns the wire
//!   op (with `rga_ops` populated) for the sync layer to gossip.
//! - `apply_remote(op)` — checks idempotency and structural conflict.
//!   For concurrent text ops on the same block the doc replays the
//!   wire `rga_ops` on its local RGA and rebuilds `block.content` from
//!   `rga.render()` — Phase 2 silent text merge. For causally-ordered
//!   ops the editor op applies directly and the RGA mirror catches up
//!   from `rga_ops`.
//!
//! The conflict surface narrows to [`Conflict::StructuralDeleteEdit`]
//! after Phase 2; concurrent same-block text edits no longer surface.
//! Concurrent `UpdateBlockContent` / `UpdateAnnotations` (whole-block
//! replacements that the RGA can't merge) still surface as
//! [`Conflict::ConcurrentBlockEdit`].

use std::collections::HashMap;

use nexus_editor::{BlockId, BlockTree, Operation};
use serde::{Deserialize, Serialize};

use crate::conflict::Conflict;
use crate::error::Result;
use crate::id::{Lamport, OpId, SiteId};
use crate::log::OpLog;
use crate::merge::{baseline_op_id, byte_to_char_pos, subop_id};
use crate::op::{affected_blocks, primary_block_id, CrdtOp};
use crate::text::{RgaText, RgaTextOp};

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

/// CRDT-aware document: the [`BlockTree`] plus its op log, conflict
/// state, and per-block RGA mirrors.
#[derive(Clone, Debug)]
pub struct CrdtDoc {
    site: SiteId,
    lamport: Lamport,
    tree: BlockTree,
    log: OpLog,
    block_meta: HashMap<BlockId, BlockMeta>,
    /// Per-block RGA mirror. Eagerly materialised at [`Self::new`] from
    /// the baseline tree content, then kept in sync with every applied
    /// op. The RGA is what enables Phase 2 silent text merge.
    rga: HashMap<BlockId, RgaText>,
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
    ///
    /// Materialises a per-block [`RgaText`] mirror for every block in
    /// the tree using deterministic synthetic [`OpId`]s
    /// ([`baseline_op_id`]). Two sites that construct a `CrdtDoc` from
    /// equal `BlockTree` content end up with identical RGAs, so
    /// concurrent ops gossiped between them converge.
    #[must_use]
    pub fn new(site: SiteId, tree: BlockTree) -> Self {
        let mut block_meta = HashMap::new();
        let mut rga = HashMap::new();
        for (&block_id, block) in &tree.blocks {
            block_meta.insert(block_id, BlockMeta::default());
            rga.insert(block_id, materialize_rga(block_id, &block.content));
        }
        Self {
            site,
            lamport: Lamport::default(),
            tree,
            log: OpLog::new(),
            block_meta,
            rga,
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
    /// [`CrdtOp`] (with `rga_ops` populated for text ops) to gossip.
    ///
    /// # Errors
    ///
    /// Propagates [`crate::CrdtError::Editor`] if the op cannot apply
    /// (e.g. it references a missing block).
    pub fn apply_local(&mut self, op: &Operation) -> Result<CrdtOp> {
        self.lamport = self.lamport.next();
        let id = OpId::new(self.site, self.lamport);

        // Translate to RGA ops *before* mutating the tree so byte→char
        // positions resolve against pre-op content.
        let rga_ops = self.author_rga_ops(op, id);

        op.apply(&mut self.tree)?;

        self.update_rga_local(op, id, &rga_ops);
        self.update_block_meta(op, id);

        let crdt = CrdtOp {
            id,
            vv_at_creation: self.log.version_vector().clone(),
            op: op.clone(),
            rga_ops,
        };
        let appended = self.log.append(crdt.clone());
        debug_assert!(appended, "fresh local op id cannot collide");
        Ok(crdt)
    }

    /// Apply a remote op. Idempotent.
    ///
    /// # Errors
    ///
    /// Returns [`crate::CrdtError::Editor`] only when the underlying
    /// tree rejects the op for reasons other than the conflict cases
    /// the CRDT detects (e.g. malformed wire data). Concurrency
    /// conflicts are returned via [`RemoteOutcome::Conflict`], not as
    /// errors.
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

        let primary = primary_block_id(&op.op);
        if is_text_op(&op.op) && self.is_concurrent_with_local(&op, primary) {
            // Phase 2 silent merge: don't apply the editor op (its
            // byte position is stale relative to local state); replay
            // the position-free `rga_ops` on the local RGA and rebuild
            // block content from the RGA.
            self.apply_rga_ops_to_block(primary, &op.rga_ops);
            self.sync_block_content_from_rga(primary);
        } else {
            // Causally-ordered or non-text op: apply the editor op
            // against the tree, then mirror RGA-affecting ops.
            op.op.apply(&mut self.tree)?;
            self.update_rga_remote(&op.op, op.id, &op.rga_ops);
        }
        self.update_block_meta(&op.op, op.id);
        let appended = self.log.append(op);
        debug_assert!(appended, "non-duplicate remote op must append");
        Ok(RemoteOutcome::Applied)
    }

    /// Borrow the per-block RGA mirror — used by Phase 4 persistence
    /// snapshots. `None` for blocks that never existed in the doc.
    #[must_use]
    pub fn block_rga(&self, block_id: BlockId) -> Option<&RgaText> {
        self.rga.get(&block_id)
    }

    /// Look at every block this op affects and decide whether it
    /// surfaces a conflict the CRDT can't resolve silently.
    ///
    /// Phase 2: structural delete-edit always surfaces. Concurrent text
    /// edits do *not* — they're handed to the RGA. Concurrent whole-
    /// block replacements (`UpdateBlockContent` / `UpdateAnnotations`)
    /// still surface — those overwrite content/annotations en masse
    /// and the RGA can't merge them.
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

        // Phase 2: text ops never surface a content conflict — the RGA
        // resolves them silently.
        if is_text_op(&remote.op) {
            return None;
        }

        // Whole-block replacements still surface. Only flag when the
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

    fn is_concurrent_with_local(&self, remote: &CrdtOp, block_id: BlockId) -> bool {
        let Some(meta) = self.block_meta.get(&block_id) else {
            return false;
        };
        let Some(local_last) = meta.last_writer else {
            return false;
        };
        !remote.vv_at_creation.contains(local_last)
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

    /// Translate a locally-authored editor op into a list of
    /// position-free RGA ops, indexed against pre-op block content.
    /// Empty for non-text ops.
    fn author_rga_ops(&self, op: &Operation, envelope: OpId) -> Vec<RgaTextOp> {
        match op {
            Operation::InsertText {
                block_id,
                pos,
                text,
                ..
            } => self.author_insert_text(*block_id, *pos, text, envelope),
            Operation::DeleteText {
                block_id,
                pos,
                deleted_text,
                ..
            } => self.author_delete_text(*block_id, *pos, deleted_text, envelope),
            _ => Vec::new(),
        }
    }

    fn author_insert_text(
        &self,
        block_id: BlockId,
        byte_pos: usize,
        text: &str,
        envelope: OpId,
    ) -> Vec<RgaTextOp> {
        let Some(block) = self.tree.get(block_id) else {
            return Vec::new();
        };
        let Some(rga) = self.rga.get(&block_id) else {
            return Vec::new();
        };
        let char_pos = byte_to_char_pos(&block.content, byte_pos);
        let mut parent = if char_pos == 0 {
            None
        } else {
            rga.op_id_at(char_pos - 1)
        };
        let mut ops = Vec::with_capacity(text.chars().count());
        for (i, ch) in text.chars().enumerate() {
            let id = subop_id(envelope, i);
            ops.push(RgaTextOp::Insert { id, parent, ch });
            parent = Some(id);
        }
        ops
    }

    fn author_delete_text(
        &self,
        block_id: BlockId,
        byte_pos: usize,
        deleted_text: &str,
        envelope: OpId,
    ) -> Vec<RgaTextOp> {
        let Some(block) = self.tree.get(block_id) else {
            return Vec::new();
        };
        let Some(rga) = self.rga.get(&block_id) else {
            return Vec::new();
        };
        let char_start = byte_to_char_pos(&block.content, byte_pos);
        let mut ops = Vec::with_capacity(deleted_text.chars().count());
        for (i, _ch) in deleted_text.chars().enumerate() {
            if let Some(target) = rga.op_id_at(char_start + i) {
                let id = subop_id(envelope, i);
                ops.push(RgaTextOp::Delete { id, target });
            }
        }
        ops
    }

    /// Mirror a locally-applied op into the RGA state. Text ops were
    /// pre-translated; structural ops adjust the RGA map directly.
    fn update_rga_local(&mut self, op: &Operation, envelope: OpId, rga_ops: &[RgaTextOp]) {
        match op {
            Operation::InsertText { block_id, .. } | Operation::DeleteText { block_id, .. } => {
                self.apply_rga_ops_to_block(*block_id, rga_ops);
            }
            Operation::InsertBlock { block, .. } => {
                self.rga
                    .entry(block.id)
                    .or_insert_with(|| materialize_rga(block.id, &block.content));
            }
            Operation::DeleteBlock { old_block, .. } => {
                self.rga.remove(&old_block.id);
            }
            Operation::UpdateBlockContent {
                id, new_content, ..
            } => {
                let new_rga = RgaText::from_chars(new_content.chars(), |i| subop_id(envelope, i));
                self.rga.insert(*id, new_rga);
            }
            Operation::ReparentBlock { .. } | Operation::UpdateAnnotations { .. } => {}
        }
    }

    /// Mirror a causally-ordered remote op into the RGA state.
    fn update_rga_remote(&mut self, op: &Operation, envelope: OpId, rga_ops: &[RgaTextOp]) {
        // Same shape as local update — the RGA only cares about the
        // op's intent, not who authored it.
        self.update_rga_local(op, envelope, rga_ops);
    }

    fn apply_rga_ops_to_block(&mut self, block_id: BlockId, ops: &[RgaTextOp]) {
        let rga = self.rga.entry(block_id).or_default();
        for op in ops {
            rga.apply(op);
        }
    }

    fn sync_block_content_from_rga(&mut self, block_id: BlockId) {
        let Some(rga) = self.rga.get(&block_id) else {
            return;
        };
        let rendered = rga.render();
        if let Some(block) = self.tree.get_mut(block_id) {
            block.content = rendered;
        }
    }
}

/// Build the baseline RGA for a block. Each character of `content`
/// gets a deterministic synthetic [`OpId`] from [`baseline_op_id`].
fn materialize_rga(block_id: BlockId, content: &str) -> RgaText {
    RgaText::from_chars(content.chars(), |i| baseline_op_id(block_id, i))
}

/// True for `InsertText` / `DeleteText` — the ops the Phase 2 RGA
/// merge handles silently.
fn is_text_op(op: &Operation) -> bool {
    matches!(
        op,
        Operation::InsertText { .. } | Operation::DeleteText { .. }
    )
}

/// True for ops that mutate the *content* of an existing block, as
/// opposed to its placement in the tree. Conflict detection only
/// flags concurrent content edits — concurrent reparents on the same
/// block are deliberately out of Phase 2 scope (see ADR 0026 §"Open
/// follow-ups").
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

    fn tree_with_block_content(content: &str) -> (BlockTree, BlockId) {
        let mut tree = empty_tree();
        let mut block = Block::new(BlockType::Paragraph);
        block.content = content.to_string();
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

    fn delete_text_op(block_id: BlockId, pos: usize, deleted: &str) -> Operation {
        Operation::DeleteText {
            block_id,
            pos,
            deleted_text: deleted.into(),
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
        // Phase 2: wire op carries one RGA op per char.
        assert_eq!(wire.rga_ops.len(), 5);
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
            rga_ops: vec![],
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
    fn concurrent_text_edits_silently_merge() {
        // Phase 2 headline test: two sites edit the same block
        // concurrently and both reach the same final content via RGA
        // — *without* surfacing a Conflict.
        let s1 = SiteId::new();
        let s2 = SiteId::new();
        let (tree, b) = empty_tree_with_block();
        let mut doc1 = CrdtDoc::new(s1, tree.clone());
        let mut doc2 = CrdtDoc::new(s2, tree);

        let op1 = doc1.apply_local(&insert_text_op(b, 0, "alpha")).unwrap();
        let op2 = doc2.apply_local(&insert_text_op(b, 0, "beta")).unwrap();

        // Both ops authored at pos 0 with the same baseline. RGA
        // tiebreak by OpId orders the two chains deterministically.
        let r1 = doc1.apply_remote(op2).unwrap();
        let r2 = doc2.apply_remote(op1).unwrap();
        assert!(matches!(r1, RemoteOutcome::Applied));
        assert!(matches!(r2, RemoteOutcome::Applied));

        // Convergence: both sites must agree.
        assert_eq!(
            doc1.tree().get(b).unwrap().content,
            doc2.tree().get(b).unwrap().content,
        );
        // No data is lost — the final string contains both inserts.
        let merged = &doc1.tree().get(b).unwrap().content;
        assert!(merged.contains("alpha"), "lost alpha: {merged}");
        assert!(merged.contains("beta"), "lost beta: {merged}");
    }

    #[test]
    fn concurrent_inserts_at_different_positions_in_baseline() {
        // Site 1 and site 2 share baseline "hello world"; each inserts
        // at a different position. Both must converge to a string that
        // contains both edits in their authored positions.
        let s1 = SiteId::new();
        let s2 = SiteId::new();
        let (tree, b) = tree_with_block_content("hello world");
        let mut doc1 = CrdtDoc::new(s1, tree.clone());
        let mut doc2 = CrdtDoc::new(s2, tree);

        // Site 1 inserts " dear" after "hello" (byte pos 5).
        let op1 = doc1.apply_local(&insert_text_op(b, 5, " dear")).unwrap();
        // Site 2 (concurrent) inserts "!" at the end (byte pos 11).
        let op2 = doc2.apply_local(&insert_text_op(b, 11, "!")).unwrap();

        doc1.apply_remote(op2).unwrap();
        doc2.apply_remote(op1).unwrap();

        let r1 = &doc1.tree().get(b).unwrap().content;
        let r2 = &doc2.tree().get(b).unwrap().content;
        assert_eq!(r1, r2, "convergence required");
        assert_eq!(r1, "hello dear world!", "expected silent merge result");
    }

    #[test]
    fn concurrent_delete_and_insert_silently_merge() {
        let s1 = SiteId::new();
        let s2 = SiteId::new();
        let (tree, b) = tree_with_block_content("hello");
        let mut doc1 = CrdtDoc::new(s1, tree.clone());
        let mut doc2 = CrdtDoc::new(s2, tree);

        // Site 1 deletes "ello" (pos 1, 4 chars).
        let op1 = doc1.apply_local(&delete_text_op(b, 1, "ello")).unwrap();
        // Site 2 (concurrent) inserts "!" at end (pos 5).
        let op2 = doc2.apply_local(&insert_text_op(b, 5, "!")).unwrap();

        doc1.apply_remote(op2).unwrap();
        doc2.apply_remote(op1).unwrap();

        let r1 = &doc1.tree().get(b).unwrap().content;
        let r2 = &doc2.tree().get(b).unwrap().content;
        assert_eq!(r1, r2, "convergence required");
        assert_eq!(r1, "h!");
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
                block_id,
                delete,
                edit,
            }) => {
                assert_eq!(block_id, b);
                assert_eq!(delete, delete_op.id);
                assert_eq!(edit.site, s2);
            }
            other => panic!("expected StructuralDeleteEdit, got {other:?}"),
        }
    }

    #[test]
    fn three_site_concurrent_text_converges() {
        // Three sites all start from the same baseline, all insert at
        // pos 0, all gossip to each other. Final state must agree.
        let sites: [SiteId; 3] = [SiteId::new(), SiteId::new(), SiteId::new()];
        let (tree, b) = tree_with_block_content("Z");
        let mut docs: Vec<CrdtDoc> =
            sites.iter().map(|&s| CrdtDoc::new(s, tree.clone())).collect();

        // Each authors its own concurrent insert.
        let ops: Vec<CrdtOp> = (0..3)
            .map(|i| {
                let payload = match i {
                    0 => "alpha",
                    1 => "beta",
                    _ => "gamma",
                };
                docs[i].apply_local(&insert_text_op(b, 0, payload)).unwrap()
            })
            .collect();

        // Everyone receives everyone else's ops.
        for (i, doc) in docs.iter_mut().enumerate() {
            for (j, op) in ops.iter().enumerate() {
                if i == j {
                    continue;
                }
                let outcome = doc.apply_remote(op.clone()).unwrap();
                assert!(matches!(outcome, RemoteOutcome::Applied));
            }
        }

        let r0 = &docs[0].tree().get(b).unwrap().content;
        let r1 = &docs[1].tree().get(b).unwrap().content;
        let r2 = &docs[2].tree().get(b).unwrap().content;
        assert_eq!(r0, r1);
        assert_eq!(r1, r2);
        for s in ["alpha", "beta", "gamma", "Z"] {
            assert!(r0.contains(s), "lost {s}: {r0}");
        }
    }

    #[test]
    fn concurrent_update_block_content_still_surfaces() {
        // UpdateBlockContent is a wholesale replacement — Phase 2 RGA
        // can't merge it. Still surfaces as ConcurrentBlockEdit.
        let s1 = SiteId::new();
        let s2 = SiteId::new();
        let (tree, b) = empty_tree_with_block();
        let mut doc1 = CrdtDoc::new(s1, tree.clone());
        let mut doc2 = CrdtDoc::new(s2, tree);

        let op1 = doc1
            .apply_local(&Operation::UpdateBlockContent {
                id: b,
                old_content: String::new(),
                new_content: "site1".into(),
                old_annotations: vec![],
                new_annotations: vec![],
            })
            .unwrap();
        let op2 = doc2
            .apply_local(&Operation::UpdateBlockContent {
                id: b,
                old_content: String::new(),
                new_content: "site2".into(),
                old_annotations: vec![],
                new_annotations: vec![],
            })
            .unwrap();

        match doc1.apply_remote(op2).unwrap() {
            RemoteOutcome::Conflict(Conflict::ConcurrentBlockEdit { block_id, .. }) => {
                assert_eq!(block_id, b);
            }
            other => panic!("expected ConcurrentBlockEdit, got {other:?}"),
        }
        // Symmetric.
        match doc2.apply_remote(op1).unwrap() {
            RemoteOutcome::Conflict(Conflict::ConcurrentBlockEdit { .. }) => {}
            other => panic!("expected ConcurrentBlockEdit, got {other:?}"),
        }
    }
}
