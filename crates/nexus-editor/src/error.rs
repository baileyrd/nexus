//! Error types for the `nexus-editor` crate.
//!
//! A single top-level [`EditorError`] covers every failure in the block
//! tree, annotations and transaction layers.

use crate::block::BlockId;

/// Crate-local result alias.
pub type Result<T> = std::result::Result<T, EditorError>;

/// Errors raised by the editor engine.
#[derive(Debug, thiserror::Error)]
pub enum EditorError {
    /// A block ID was referenced but is not present in the tree.
    #[error("block not found: {0}")]
    BlockNotFound(BlockId),

    /// An inline range (annotation or text edit) is out of bounds for
    /// the block's content.
    #[error("invalid range in block {block_id}: {start}..{end} (content len {len})")]
    InvalidRange {
        /// Block whose content was being addressed.
        block_id: BlockId,
        /// Inclusive start byte.
        start: usize,
        /// Exclusive end byte.
        end: usize,
        /// Length of the block's `content` string, in bytes.
        len: usize,
    },

    /// A tree invariant (parent/child link, index, orphan) was violated.
    #[error("tree invariant violated: {0}")]
    InvalidTree(String),

    /// A transaction is malformed or inconsistent with the current tree.
    #[error("transaction invalid: {0}")]
    TransactionInvalid(String),

    /// Undo/redo navigation failed (e.g. goto an unreachable index).
    #[error("undo/redo failed: {0}")]
    UndoRedo(String),
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn block_not_found_display_includes_id() {
        let id = Uuid::new_v4();
        let msg = format!("{}", EditorError::BlockNotFound(id));
        assert!(msg.contains(&id.to_string()));
    }

    #[test]
    fn invalid_range_display_includes_numbers() {
        let id = Uuid::new_v4();
        let msg = format!(
            "{}",
            EditorError::InvalidRange {
                block_id: id,
                start: 3,
                end: 9,
                len: 5,
            }
        );
        assert!(msg.contains("3..9"));
        assert!(msg.contains("len 5"));
    }

    #[test]
    fn invalid_tree_wraps_message() {
        let e = EditorError::InvalidTree("orphan block".into());
        assert!(format!("{e}").contains("orphan block"));
    }
}
