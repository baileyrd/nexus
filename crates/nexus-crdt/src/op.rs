//! [`CrdtOp`]: a [`nexus_editor::Operation`] wrapped with the CRDT
//! envelope (op id and the version vector observed at authoring time).

use nexus_editor::{BlockId, Operation};
use serde::{Deserialize, Serialize};

use crate::id::{OpId, VersionVector};
use crate::text::RgaTextOp;

/// A CRDT-tagged edit. The envelope adds:
///
/// - `id`: who authored it (`SiteId`) and when (`Lamport`).
/// - `vv_at_creation`: the authoring site's version vector immediately
///   *before* this op was authored. Two ops `A` and `B` are concurrent
///   iff `B.id ∉ A.vv_at_creation` and `A.id ∉ B.vv_at_creation`. This
///   is the standard vector-clock CRDT causality test.
/// - `rga_ops` (Phase 2): the position-free RGA translation of `op`,
///   computed by the authoring site against its own RGA state.
///   Receivers replay these on their own per-block RGA mirror so
///   concurrent text edits converge silently — see ADR 0026 §"Phase 2".
///   Empty for non-text ops and for legacy/Phase-1 wire payloads.
///
/// The wrapped [`Operation`] retains its full self-reversal payload
/// (`pre_annotations`, `deleted_text`, etc.) so undo on the receiving
/// site is just `Operation::reverse`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CrdtOp {
    /// Globally-unique authoring identity.
    pub id: OpId,
    /// Causality witness — the authoring site's VV before this op.
    pub vv_at_creation: VersionVector,
    /// The wrapped editor primitive.
    pub op: Operation,
    /// Per-character RGA translation of `op`. Authored at apply-local
    /// time so peers can replay the edit position-free.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rga_ops: Vec<RgaTextOp>,
}

/// Return the block id this op primarily targets, used to bucket ops
/// for conflict detection. For structural ops that affect a parent and
/// a child (insert/delete/reparent), this returns the *moved* block.
#[must_use]
pub fn primary_block_id(op: &Operation) -> BlockId {
    match op {
        Operation::InsertText { block_id, .. }
        | Operation::DeleteText { block_id, .. }
        | Operation::UpdateAnnotations { block_id, .. } => *block_id,
        Operation::InsertBlock { block, .. } => block.id,
        Operation::DeleteBlock { old_block, .. } => old_block.id,
        Operation::ReparentBlock { id, .. } | Operation::UpdateBlockContent { id, .. } => *id,
    }
}

/// Every block id whose state is read or written by this op. Used by
/// the conflict detector to spot e.g. a `DeleteBlock` racing with a
/// child `InsertText` on the same `block_id`.
#[must_use]
pub fn affected_blocks(op: &Operation) -> Vec<BlockId> {
    match op {
        Operation::InsertText { block_id, .. }
        | Operation::DeleteText { block_id, .. }
        | Operation::UpdateAnnotations { block_id, .. } => vec![*block_id],
        Operation::UpdateBlockContent { id, .. } => vec![*id],
        Operation::InsertBlock {
            block, parent_id, ..
        } => parent_id.map_or_else(|| vec![block.id], |p| vec![p, block.id]),
        Operation::DeleteBlock {
            old_block,
            was_parent_id,
            ..
        } => was_parent_id.map_or_else(|| vec![old_block.id], |p| vec![p, old_block.id]),
        Operation::ReparentBlock {
            id,
            old_parent_id,
            new_parent_id,
            ..
        } => {
            let mut v = vec![*id];
            if let Some(p) = *old_parent_id {
                v.push(p);
            }
            if let Some(p) = *new_parent_id {
                v.push(p);
            }
            v
        }
    }
}

#[cfg(test)]
mod tests {
    use nexus_editor::{Block, BlockType};
    use uuid::Uuid;

    use super::*;

    #[test]
    fn primary_block_id_for_text_ops_returns_target() {
        let block = Uuid::new_v4();
        let op = Operation::InsertText {
            block_id: block,
            pos: 0,
            text: "hi".into(),
            pre_annotations: vec![],
        };
        assert_eq!(primary_block_id(&op), block);
    }

    #[test]
    fn affected_blocks_for_reparent_includes_both_parents() {
        let id = Uuid::new_v4();
        let old = Uuid::new_v4();
        let new = Uuid::new_v4();
        let op = Operation::ReparentBlock {
            id,
            old_parent_id: Some(old),
            old_index_in_parent: 0,
            new_parent_id: Some(new),
            new_index_in_parent: 0,
        };
        let mut blocks = affected_blocks(&op);
        blocks.sort();
        let mut expected = vec![id, old, new];
        expected.sort();
        assert_eq!(blocks, expected);
    }

    #[test]
    fn affected_blocks_for_root_insert_omits_parent() {
        let block = Block::new(BlockType::Paragraph);
        let op = Operation::InsertBlock {
            block: block.clone(),
            parent_id: None,
            index_in_parent: 0,
        };
        assert_eq!(affected_blocks(&op), vec![block.id]);
    }
}
