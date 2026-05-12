//! Transactions and operations (PRD 08 §5.1).
//!
//! A [`Transaction`] is an atomic, reversible group of [`Operation`]s.
//! Every operation carries enough data to reverse itself without
//! reading from the tree — `DeleteText` includes the deleted text and
//! `DeleteBlock` includes the full [`Block`] plus its original parent
//! and index. (This is a deliberate deviation from the PRD sample code,
//! flagged in the crate plan.)

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::annotation::{adjust_annotations, Annotation};
use crate::block::{now_ms, Block, BlockId};
use crate::error::{EditorError, Result};
use crate::tree::BlockTree;

// ── Operation ─────────────────────────────────────────────────────────────────

/// A single reversible edit primitive.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Operation {
    /// Insert `text` into the named block's content at `pos`. Shifts
    /// annotations forward per [`adjust_annotations`]; reversal
    /// restores `pre_annotations` verbatim.
    InsertText {
        /// Target block.
        block_id: BlockId,
        /// Byte position at which to insert.
        pos: usize,
        /// Text to insert.
        text: String,
        /// Block's annotations before this op was applied.
        pre_annotations: Vec<Annotation>,
    },

    /// Delete `deleted_text` from the named block's content starting at
    /// `pos`. The deleted text and the prior annotations are stored so
    /// the operation can reverse itself exactly (even when annotations
    /// fully collapsed).
    DeleteText {
        /// Target block.
        block_id: BlockId,
        /// Byte position of the leftmost byte removed.
        pos: usize,
        /// Exact text that was removed (for round-trip safety).
        deleted_text: String,
        /// Block's annotations before this op was applied.
        pre_annotations: Vec<Annotation>,
    },

    /// Insert a leaf block into the tree. To insert a subtree, emit
    /// children via additional `InsertBlock` ops in pre-order.
    InsertBlock {
        /// Block to insert (must have `children` empty).
        block: Block,
        /// Parent block (or `None` for root placement).
        parent_id: Option<BlockId>,
        /// Target index within the parent's children or `root_blocks`.
        index_in_parent: usize,
    },

    /// Remove a leaf block from the tree, recording enough state to
    /// re-insert it on reverse.
    DeleteBlock {
        /// The block that was removed.
        old_block: Block,
        /// The parent it was detached from.
        was_parent_id: Option<BlockId>,
        /// Its previous index.
        was_index_in_parent: usize,
    },

    /// Move a block (and its subtree) to a new location.
    ReparentBlock {
        /// Block being moved.
        id: BlockId,
        /// Previous parent.
        old_parent_id: Option<BlockId>,
        /// Previous index within the old parent.
        old_index_in_parent: usize,
        /// Target parent.
        new_parent_id: Option<BlockId>,
        /// Target index within the new parent.
        new_index_in_parent: usize,
    },

    /// Replace the plain-text content and annotations of a block.
    UpdateBlockContent {
        /// Target block.
        id: BlockId,
        /// Content before the edit.
        old_content: String,
        /// Content after the edit.
        new_content: String,
        /// Annotations before the edit.
        old_annotations: Vec<Annotation>,
        /// Annotations after the edit.
        new_annotations: Vec<Annotation>,
    },

    /// Replace the annotations of a block, leaving content untouched.
    UpdateAnnotations {
        /// Target block.
        block_id: BlockId,
        /// Annotations before the edit.
        old_annotations: Vec<Annotation>,
        /// Annotations after the edit.
        new_annotations: Vec<Annotation>,
    },
}

impl Operation {
    /// Apply this operation to `tree`.
    ///
    /// # Errors
    /// - [`EditorError::BlockNotFound`] if a referenced block is missing.
    /// - [`EditorError::InvalidRange`] if a text position is out of bounds.
    /// - [`EditorError::TransactionInvalid`] if the delete range does
    ///   not match the stored `deleted_text`.
    /// - [`EditorError::InvalidTree`] for tree-shape violations.
    pub fn apply(&self, tree: &mut BlockTree) -> Result<()> {
        match self {
            Self::InsertText {
                block_id,
                pos,
                text,
                ..
            } => apply_insert_text(tree, *block_id, *pos, text),
            Self::DeleteText {
                block_id,
                pos,
                deleted_text,
                ..
            } => apply_delete_text(tree, *block_id, *pos, deleted_text),
            Self::InsertBlock {
                block,
                parent_id,
                index_in_parent,
            } => {
                tree.insert(block.clone(), *parent_id, *index_in_parent)?;
                Ok(())
            }
            Self::DeleteBlock { old_block, .. } => {
                tree.remove(old_block.id)?;
                Ok(())
            }
            Self::ReparentBlock {
                id,
                new_parent_id,
                new_index_in_parent,
                ..
            } => tree.reparent(*id, *new_parent_id, *new_index_in_parent),
            Self::UpdateBlockContent {
                id,
                new_content,
                new_annotations,
                ..
            } => {
                let block = tree.get_mut(*id).ok_or(EditorError::BlockNotFound(*id))?;
                block.content.clone_from(new_content);
                block.annotations.clone_from(new_annotations);
                Ok(())
            }
            Self::UpdateAnnotations {
                block_id,
                new_annotations,
                ..
            } => {
                let block = tree
                    .get_mut(*block_id)
                    .ok_or(EditorError::BlockNotFound(*block_id))?;
                block.annotations.clone_from(new_annotations);
                Ok(())
            }
        }
    }

    /// Compute the inverse of this operation as a *new* [`Operation`]
    /// against `tree` in its **post-apply** state, without mutating
    /// the tree.
    ///
    /// Used by the BL-074 publisher to author CRDT envelopes for
    /// undo / redo: peers see the inverse as a fresh local op and
    /// stay convergent with the local site's intent. The `tree`
    /// argument should be the publisher's mirror at the moment the
    /// undo fires (i.e., still showing the result of the original
    /// transaction); the inverse op authored against that state can
    /// then be `apply_local`'d to roll back the mirror.
    ///
    /// # Errors
    /// - [`EditorError::BlockNotFound`] if a referenced block has
    ///   already been removed from `tree`.
    pub fn inverse(&self, tree: &BlockTree) -> Result<Self> {
        match self {
            Self::InsertText {
                block_id,
                pos,
                text,
                ..
            } => {
                let block = tree
                    .get(*block_id)
                    .ok_or(EditorError::BlockNotFound(*block_id))?;
                Ok(Self::DeleteText {
                    block_id: *block_id,
                    pos: *pos,
                    deleted_text: text.clone(),
                    pre_annotations: block.annotations.clone(),
                })
            }
            Self::DeleteText {
                block_id,
                pos,
                deleted_text,
                ..
            } => {
                let block = tree
                    .get(*block_id)
                    .ok_or(EditorError::BlockNotFound(*block_id))?;
                Ok(Self::InsertText {
                    block_id: *block_id,
                    pos: *pos,
                    text: deleted_text.clone(),
                    pre_annotations: block.annotations.clone(),
                })
            }
            Self::InsertBlock {
                block,
                parent_id,
                index_in_parent,
            } => {
                // Capture the block's current state from the tree —
                // it may have been mutated by later ops in the same
                // transaction even though InsertBlock authored an
                // empty leaf.
                let live = tree
                    .get(block.id)
                    .cloned()
                    .unwrap_or_else(|| block.clone());
                Ok(Self::DeleteBlock {
                    old_block: live,
                    was_parent_id: *parent_id,
                    was_index_in_parent: *index_in_parent,
                })
            }
            Self::DeleteBlock {
                old_block,
                was_parent_id,
                was_index_in_parent,
            } => Ok(Self::InsertBlock {
                block: old_block.clone(),
                parent_id: *was_parent_id,
                index_in_parent: *was_index_in_parent,
            }),
            Self::ReparentBlock {
                id,
                old_parent_id,
                old_index_in_parent,
                new_parent_id,
                new_index_in_parent,
            } => Ok(Self::ReparentBlock {
                id: *id,
                old_parent_id: *new_parent_id,
                old_index_in_parent: *new_index_in_parent,
                new_parent_id: *old_parent_id,
                new_index_in_parent: *old_index_in_parent,
            }),
            Self::UpdateBlockContent {
                id,
                old_content,
                new_content,
                old_annotations,
                new_annotations,
            } => Ok(Self::UpdateBlockContent {
                id: *id,
                old_content: new_content.clone(),
                new_content: old_content.clone(),
                old_annotations: new_annotations.clone(),
                new_annotations: old_annotations.clone(),
            }),
            Self::UpdateAnnotations {
                block_id,
                old_annotations,
                new_annotations,
            } => Ok(Self::UpdateAnnotations {
                block_id: *block_id,
                old_annotations: new_annotations.clone(),
                new_annotations: old_annotations.clone(),
            }),
        }
    }

    /// Reverse this operation, restoring the state that existed before
    /// the matching [`Operation::apply`].
    ///
    /// # Errors
    /// Same classes as [`Operation::apply`].
    pub fn reverse(&self, tree: &mut BlockTree) -> Result<()> {
        match self {
            Self::InsertText {
                block_id,
                pos,
                text,
                pre_annotations,
            } => {
                apply_delete_text(tree, *block_id, *pos, text)?;
                let block = tree
                    .get_mut(*block_id)
                    .ok_or(EditorError::BlockNotFound(*block_id))?;
                block.annotations.clone_from(pre_annotations);
                Ok(())
            }
            Self::DeleteText {
                block_id,
                pos,
                deleted_text,
                pre_annotations,
            } => {
                apply_insert_text(tree, *block_id, *pos, deleted_text)?;
                let block = tree
                    .get_mut(*block_id)
                    .ok_or(EditorError::BlockNotFound(*block_id))?;
                block.annotations.clone_from(pre_annotations);
                Ok(())
            }
            Self::InsertBlock { block, .. } => {
                tree.remove(block.id)?;
                Ok(())
            }
            Self::DeleteBlock {
                old_block,
                was_parent_id,
                was_index_in_parent,
            } => {
                tree.insert(old_block.clone(), *was_parent_id, *was_index_in_parent)?;
                Ok(())
            }
            Self::ReparentBlock {
                id,
                old_parent_id,
                old_index_in_parent,
                new_parent_id,
                new_index_in_parent,
            } => {
                // [`BlockTree::reparent`] treats `new_index` as a pre-detach
                // position and auto-subtracts 1 on forward same-parent
                // moves. That adjustment is one-sided — reversing a
                // *backward* same-parent move (new_index < old_index)
                // means replaying a forward same-parent move in reverse,
                // which needs `old_index + 1` to land in the right slot
                // once `id` is unlinked. The asymmetry is invisible for
                // cross-parent reparents, which is why the existing
                // reparent_roundtrip test passes.
                let reverse_target = if old_parent_id == new_parent_id
                    && new_index_in_parent < old_index_in_parent
                {
                    *old_index_in_parent + 1
                } else {
                    *old_index_in_parent
                };
                tree.reparent(*id, *old_parent_id, reverse_target)
            }
            Self::UpdateBlockContent {
                id,
                old_content,
                old_annotations,
                ..
            } => {
                let block = tree.get_mut(*id).ok_or(EditorError::BlockNotFound(*id))?;
                block.content.clone_from(old_content);
                block.annotations.clone_from(old_annotations);
                Ok(())
            }
            Self::UpdateAnnotations {
                block_id,
                old_annotations,
                ..
            } => {
                let block = tree
                    .get_mut(*block_id)
                    .ok_or(EditorError::BlockNotFound(*block_id))?;
                block.annotations.clone_from(old_annotations);
                Ok(())
            }
        }
    }
}

fn apply_insert_text(
    tree: &mut BlockTree,
    block_id: BlockId,
    pos: usize,
    text: &str,
) -> Result<()> {
    let block = tree
        .get_mut(block_id)
        .ok_or(EditorError::BlockNotFound(block_id))?;
    if pos > block.content.len() {
        return Err(EditorError::InvalidRange {
            block_id,
            start: pos,
            end: pos,
            len: block.content.len(),
        });
    }
    block.content.insert_str(pos, text);
    let len = isize::try_from(text.len()).map_err(|_| {
        EditorError::TransactionInvalid("insert text length overflows isize".into())
    })?;
    adjust_annotations(&mut block.annotations, pos, len);
    Ok(())
}

fn apply_delete_text(
    tree: &mut BlockTree,
    block_id: BlockId,
    pos: usize,
    deleted_text: &str,
) -> Result<()> {
    let block = tree
        .get_mut(block_id)
        .ok_or(EditorError::BlockNotFound(block_id))?;
    let end = pos.saturating_add(deleted_text.len());
    if end > block.content.len() {
        return Err(EditorError::InvalidRange {
            block_id,
            start: pos,
            end,
            len: block.content.len(),
        });
    }
    if &block.content[pos..end] != deleted_text {
        return Err(EditorError::TransactionInvalid(format!(
            "delete-text mismatch at {pos}..{end} in block {block_id}"
        )));
    }
    block.content.replace_range(pos..end, "");
    let len = isize::try_from(deleted_text.len()).map_err(|_| {
        EditorError::TransactionInvalid("delete text length overflows isize".into())
    })?;
    adjust_annotations(&mut block.annotations, pos, -len);
    Ok(())
}

// ── Transaction ───────────────────────────────────────────────────────────────

/// A group of operations applied atomically.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transaction {
    /// Unique id (used by [`crate::UndoTree`] to identify history nodes).
    pub id: Uuid,
    /// Operations, applied in declaration order.
    pub operations: Vec<Operation>,
    /// Unix epoch milliseconds.
    pub created_at: i64,
    /// Context about this transaction.
    pub metadata: TransactionMetadata,
}

impl Transaction {
    /// Build a transaction from a list of operations.
    #[must_use]
    pub fn new(operations: Vec<Operation>, metadata: TransactionMetadata) -> Self {
        Self {
            id: Uuid::new_v4(),
            operations,
            created_at: now_ms(),
            metadata,
        }
    }

    /// Apply every operation in order.
    ///
    /// # Errors
    /// Propagates the first operation failure. Already-applied
    /// operations are **not** rolled back automatically — the caller
    /// owns recovery.
    pub fn apply(&self, tree: &mut BlockTree) -> Result<()> {
        for op in &self.operations {
            op.apply(tree)?;
        }
        tree.metadata.updated_at = now_ms();
        Ok(())
    }

    /// Reverse every operation in reverse order.
    ///
    /// # Errors
    /// Propagates the first operation failure.
    pub fn reverse(&self, tree: &mut BlockTree) -> Result<()> {
        for op in self.operations.iter().rev() {
            op.reverse(tree)?;
        }
        tree.metadata.updated_at = now_ms();
        Ok(())
    }

    /// Build a single-op [`Operation::ReparentBlock`] transaction that
    /// moves `id` under `new_parent` at `new_index`, auto-filling the
    /// block's current parent + index from `tree`.
    ///
    /// This is the canonical "move block" entry point for block-drag UX
    /// (docs/archive/notion-block-ux-plan.md Phase 3). Single op = single undo step,
    /// so `ctrl+z` after a drag reverses the whole move atomically
    /// instead of re-inserting and re-deleting one edge at a time.
    ///
    /// `metadata` defaults to
    /// `BlockOperation { op: Move { direction: "reorder" } }` when
    /// `None`, which is the telemetry shape the existing undo UI already
    /// groups together. Callers that want a different `UserAction`
    /// (e.g. drag-drop) should pass one explicitly.
    ///
    /// # Errors
    ///
    /// Returns [`EditorError::BlockNotFound`] if `id` doesn't resolve
    /// in `tree`.
    pub fn move_block(
        tree: &BlockTree,
        id: BlockId,
        new_parent: Option<BlockId>,
        new_index: usize,
        metadata: Option<TransactionMetadata>,
    ) -> Result<Self> {
        let block = tree.get(id).ok_or(EditorError::BlockNotFound(id))?;
        let op = Operation::ReparentBlock {
            id,
            old_parent_id: block.parent_id,
            old_index_in_parent: block.index_in_parent,
            new_parent_id: new_parent,
            new_index_in_parent: new_index,
        };
        let metadata = metadata.unwrap_or(TransactionMetadata {
            user_action: UserAction::BlockOperation {
                op: BlockOp::Move {
                    direction: "reorder".to_string(),
                },
            },
            source: TransactionSource::User,
            ai_edit: false,
        });
        Ok(Self::new(vec![op], metadata))
    }
}

// ── Transaction metadata ──────────────────────────────────────────────────────

/// Metadata describing the origin and intent of a transaction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransactionMetadata {
    /// What the user was doing when this was emitted.
    pub user_action: UserAction,
    /// Who triggered the transaction.
    pub source: TransactionSource,
    /// `true` if generated by AI-assisted editing.
    pub ai_edit: bool,
}

impl Default for TransactionMetadata {
    fn default() -> Self {
        Self {
            user_action: UserAction::Keystroke,
            source: TransactionSource::User,
            ai_edit: false,
        }
    }
}

/// High-level user gesture that produced a transaction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum UserAction {
    /// Single character / keystroke edit.
    Keystroke,
    /// Paste from clipboard.
    Paste,
    /// Delete key / cut operation.
    Delete,
    /// Slash command execution.
    SlashCommand {
        /// Command identifier (e.g. `"heading1"`).
        command: String,
    },
    /// Block-tree operation.
    BlockOperation {
        /// Specific block manipulation.
        op: BlockOp,
    },
    /// Drag-and-drop gesture.
    DragDrop,
}

/// Kinds of block-tree operations the user can invoke.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum BlockOp {
    /// Create a new block of a given type.
    Create {
        /// Human-readable block type name.
        block_type: String,
    },
    /// Delete a block.
    Delete,
    /// Move a block within the tree.
    Move {
        /// `"up" | "down" | "left" | "right"`.
        direction: String,
    },
    /// Transform a block's type in place.
    Transform {
        /// Source block type name.
        from_type: String,
        /// Target block type name.
        to_type: String,
    },
    /// Increase nesting depth.
    Indent,
    /// Decrease nesting depth.
    Unindent,
}

/// Who originated a transaction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionSource {
    /// Direct user input.
    User,
    /// AI-assisted edit.
    Ai,
    /// Sync/replication from another client.
    Sync,
    /// System-generated (e.g. automatic formatting).
    System,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotation::{Annotation, AnnotationType};
    use crate::block::{Block, BlockType, DocumentMetadata};

    fn para(text: &str) -> Block {
        Block::new(BlockType::Paragraph).with_content(text)
    }

    /// Compare the mutable parts of a tree (not metadata timestamps).
    fn trees_structurally_equal(a: &BlockTree, b: &BlockTree) -> bool {
        a.blocks == b.blocks && a.root_blocks == b.root_blocks
    }

    fn metadata() -> TransactionMetadata {
        TransactionMetadata::default()
    }

    // ── InsertText / DeleteText ──

    #[test]
    fn insert_text_applies_and_reverses() {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let id = tree.insert(para("hello"), None, 0).unwrap();
        let snapshot = tree.clone();

        let op = Operation::InsertText {
            block_id: id,
            pos: 5,
            text: " world".into(),
            pre_annotations: vec![],
        };
        op.apply(&mut tree).unwrap();
        assert_eq!(tree.get(id).unwrap().content, "hello world");

        op.reverse(&mut tree).unwrap();
        assert!(trees_structurally_equal(&tree, &snapshot));
    }

    #[test]
    fn insert_text_shifts_downstream_annotations() {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let pre = vec![Annotation {
            start: 3,
            end: 6,
            ty: AnnotationType::Bold,
        }];
        let mut block = para("abcdefg");
        block.annotations = pre.clone();
        let id = tree.insert(block, None, 0).unwrap();

        Operation::InsertText {
            block_id: id,
            pos: 1,
            text: "XX".into(),
            pre_annotations: pre,
        }
        .apply(&mut tree)
        .unwrap();

        let ann = &tree.get(id).unwrap().annotations[0];
        assert_eq!((ann.start, ann.end), (5, 8));
    }

    #[test]
    fn delete_text_roundtrip_restores_annotations() {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let pre = vec![Annotation {
            start: 6,
            end: 11,
            ty: AnnotationType::Bold,
        }];
        let mut block = para("hello world");
        block.annotations = pre.clone();
        let id = tree.insert(block, None, 0).unwrap();
        let snapshot = tree.clone();

        let op = Operation::DeleteText {
            block_id: id,
            pos: 5,
            deleted_text: " world".into(),
            pre_annotations: pre,
        };
        op.apply(&mut tree).unwrap();
        assert_eq!(tree.get(id).unwrap().content, "hello");
        // Bold annotation collapsed because its text was deleted.
        assert!(tree.get(id).unwrap().annotations[0].is_empty());

        op.reverse(&mut tree).unwrap();
        assert!(trees_structurally_equal(&tree, &snapshot));
    }

    #[test]
    fn delete_text_mismatch_errors() {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let id = tree.insert(para("hello"), None, 0).unwrap();
        let op = Operation::DeleteText {
            block_id: id,
            pos: 0,
            deleted_text: "HELLO".into(),
            pre_annotations: vec![],
        };
        assert!(matches!(
            op.apply(&mut tree),
            Err(EditorError::TransactionInvalid(_))
        ));
    }

    #[test]
    fn insert_text_out_of_range_errors() {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let id = tree.insert(para("hi"), None, 0).unwrap();
        let op = Operation::InsertText {
            block_id: id,
            pos: 100,
            text: "x".into(),
            pre_annotations: vec![],
        };
        assert!(matches!(
            op.apply(&mut tree),
            Err(EditorError::InvalidRange { .. })
        ));
    }

    // ── Insert / Delete block ──

    #[test]
    fn insert_block_roundtrip() {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let snapshot = tree.clone();
        let block = para("new");
        let op = Operation::InsertBlock {
            block: block.clone(),
            parent_id: None,
            index_in_parent: 0,
        };
        op.apply(&mut tree).unwrap();
        assert_eq!(tree.root_blocks, vec![block.id]);
        op.reverse(&mut tree).unwrap();
        assert!(trees_structurally_equal(&tree, &snapshot));
    }

    #[test]
    fn delete_block_roundtrip() {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let id = tree.insert(para("bye"), None, 0).unwrap();
        let snapshot = tree.clone();
        let old_block = tree.get(id).unwrap().clone();
        let op = Operation::DeleteBlock {
            old_block: old_block.clone(),
            was_parent_id: None,
            was_index_in_parent: 0,
        };
        op.apply(&mut tree).unwrap();
        assert!(tree.is_empty());
        op.reverse(&mut tree).unwrap();
        assert!(trees_structurally_equal(&tree, &snapshot));
    }

    // ── Reparent ──

    #[test]
    fn reparent_roundtrip() {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let p1 = tree.insert(para("p1"), None, 0).unwrap();
        let p2 = tree.insert(para("p2"), None, 1).unwrap();
        let a = tree.insert(para("a"), Some(p1), 0).unwrap();
        let snapshot = tree.clone();
        let op = Operation::ReparentBlock {
            id: a,
            old_parent_id: Some(p1),
            old_index_in_parent: 0,
            new_parent_id: Some(p2),
            new_index_in_parent: 0,
        };
        op.apply(&mut tree).unwrap();
        assert_eq!(tree.get(a).unwrap().parent_id, Some(p2));
        op.reverse(&mut tree).unwrap();
        assert!(trees_structurally_equal(&tree, &snapshot));
    }

    // ── Update content / annotations ──

    #[test]
    fn update_block_content_roundtrip() {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let id = tree
            .insert(para("old").with_annotations(vec![]), None, 0)
            .unwrap();
        let snapshot = tree.clone();
        let op = Operation::UpdateBlockContent {
            id,
            old_content: "old".into(),
            new_content: "new".into(),
            old_annotations: vec![],
            new_annotations: vec![Annotation {
                start: 0,
                end: 3,
                ty: AnnotationType::Bold,
            }],
        };
        op.apply(&mut tree).unwrap();
        assert_eq!(tree.get(id).unwrap().content, "new");
        assert_eq!(tree.get(id).unwrap().annotations.len(), 1);
        op.reverse(&mut tree).unwrap();
        assert!(trees_structurally_equal(&tree, &snapshot));
    }

    #[test]
    fn update_annotations_roundtrip() {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let id = tree.insert(para("xyz"), None, 0).unwrap();
        let snapshot = tree.clone();
        let op = Operation::UpdateAnnotations {
            block_id: id,
            old_annotations: vec![],
            new_annotations: vec![Annotation {
                start: 0,
                end: 3,
                ty: AnnotationType::Italic,
            }],
        };
        op.apply(&mut tree).unwrap();
        assert_eq!(tree.get(id).unwrap().annotations.len(), 1);
        op.reverse(&mut tree).unwrap();
        assert!(trees_structurally_equal(&tree, &snapshot));
    }

    // ── Compound Transaction ──

    #[test]
    fn compound_transaction_applies_in_order_reverses_in_reverse() {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let id = tree.insert(para("a"), None, 0).unwrap();
        let snapshot = tree.clone();

        let tx = Transaction::new(
            vec![
                Operation::InsertText {
                    block_id: id,
                    pos: 1,
                    text: "b".into(),
                    pre_annotations: vec![],
                },
                Operation::InsertText {
                    block_id: id,
                    pos: 2,
                    text: "c".into(),
                    pre_annotations: vec![],
                },
            ],
            metadata(),
        );
        tx.apply(&mut tree).unwrap();
        assert_eq!(tree.get(id).unwrap().content, "abc");
        tx.reverse(&mut tree).unwrap();
        assert!(trees_structurally_equal(&tree, &snapshot));
    }

    #[test]
    fn reparent_same_parent_backward_roundtrip() {
        // Regression: reversing a backward same-parent move used to land
        // the block one slot short because tree.reparent's pre-detach
        // auto-adjust is one-sided.
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let a = tree.insert(para("a"), None, 0).unwrap();
        let b = tree.insert(para("b"), None, 1).unwrap();
        let c = tree.insert(para("c"), None, 2).unwrap();
        let snapshot = tree.clone();

        let op = Operation::ReparentBlock {
            id: c,
            old_parent_id: None,
            old_index_in_parent: 2,
            new_parent_id: None,
            new_index_in_parent: 0,
        };
        op.apply(&mut tree).unwrap();
        assert_eq!(tree.root_blocks, vec![c, a, b]);
        op.reverse(&mut tree).unwrap();
        assert!(trees_structurally_equal(&tree, &snapshot));
    }

    #[test]
    fn reparent_same_parent_forward_roundtrip() {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let a = tree.insert(para("a"), None, 0).unwrap();
        let b = tree.insert(para("b"), None, 1).unwrap();
        let c = tree.insert(para("c"), None, 2).unwrap();
        let snapshot = tree.clone();

        let op = Operation::ReparentBlock {
            id: a,
            old_parent_id: None,
            old_index_in_parent: 0,
            new_parent_id: None,
            new_index_in_parent: 3,
        };
        op.apply(&mut tree).unwrap();
        assert_eq!(tree.root_blocks, vec![b, c, a]);
        op.reverse(&mut tree).unwrap();
        assert!(trees_structurally_equal(&tree, &snapshot));
    }

    #[test]
    fn move_block_constructor_autofills_old_location() {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let a = tree.insert(para("a"), None, 0).unwrap();
        let b = tree.insert(para("b"), None, 1).unwrap();
        let c = tree.insert(para("c"), None, 2).unwrap();
        let snapshot = tree.clone();

        // Move c to position 0: [c, a, b].
        let tx = Transaction::move_block(&tree, c, None, 0, None).unwrap();
        assert_eq!(tx.operations.len(), 1, "move is a single op");
        tx.apply(&mut tree).unwrap();
        assert_eq!(tree.root_blocks, vec![c, a, b]);

        // One undo reverses the whole move.
        tx.reverse(&mut tree).unwrap();
        assert!(trees_structurally_equal(&tree, &snapshot));
    }

    #[test]
    fn move_block_reports_missing_id() {
        let tree = BlockTree::new(DocumentMetadata::empty());
        let err = Transaction::move_block(&tree, Uuid::new_v4(), None, 0, None).unwrap_err();
        assert!(matches!(err, EditorError::BlockNotFound(_)));
    }

    #[test]
    fn move_block_default_metadata_tags_reorder() {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let a = tree.insert(para("a"), None, 0).unwrap();
        let _b = tree.insert(para("b"), None, 1).unwrap();
        let tx = Transaction::move_block(&tree, a, None, 1, None).unwrap();
        match &tx.metadata.user_action {
            UserAction::BlockOperation { op: BlockOp::Move { direction } } => {
                assert_eq!(direction, "reorder");
            }
            other => panic!("expected BlockOperation::Move, got {other:?}"),
        }
    }

    #[test]
    fn block_not_found_errors_from_text_ops() {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let op = Operation::InsertText {
            block_id: Uuid::new_v4(),
            pos: 0,
            text: "x".into(),
            pre_annotations: vec![],
        };
        assert!(matches!(
            op.apply(&mut tree),
            Err(EditorError::BlockNotFound(_))
        ));
    }
}
