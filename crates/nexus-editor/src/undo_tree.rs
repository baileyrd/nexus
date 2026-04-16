//! Branching undo history (PRD 08 §5.2).
//!
//! Each [`crate::Transaction`] becomes a node in an undo tree. Unlike a
//! linear stack, executing after an undo creates a new branch rather
//! than truncating the future; [`UndoTree::goto`] can walk across
//! branches through the lowest common ancestor.
//!
//! A "virtual root" sits above the tree: the position before any
//! transaction has been executed. It is represented as
//! `current == None` and `parent` having no entry for direct children
//! of the virtual root.

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{EditorError, Result};
use crate::transaction::Transaction;
use crate::tree::BlockTree;

/// Branching undo history.
#[derive(Clone, Debug, Default)]
pub struct UndoTree {
    transactions: Vec<Arc<Transaction>>,
    current: Option<usize>,
    /// `child_idx → parent_idx`. Missing entries are direct children
    /// of the virtual root (i.e. their parent is `None`).
    parent: HashMap<usize, usize>,
    /// `parent_idx (or virtual root) → child_indices` in insertion
    /// order.
    children: HashMap<Option<usize>, Vec<usize>>,
}

impl UndoTree {
    /// Empty history.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of transactions ever executed (including across branches).
    #[must_use]
    pub fn len(&self) -> usize {
        self.transactions.len()
    }

    /// `true` if no transactions have been executed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }

    /// The current transaction index, or `None` at the virtual root.
    #[must_use]
    pub fn current(&self) -> Option<usize> {
        self.current
    }

    /// Immutable access to all recorded transactions.
    #[must_use]
    pub fn transactions(&self) -> &[Arc<Transaction>] {
        &self.transactions
    }

    /// Apply `tx` to `tree`, record it as a child of the current
    /// position, and advance the cursor.
    ///
    /// # Errors
    /// Propagates failures from [`Transaction::apply`]. On error the
    /// history is not mutated.
    pub fn execute(&mut self, tx: Transaction, tree: &mut BlockTree) -> Result<()> {
        tx.apply(tree)?;
        let idx = self.transactions.len();
        self.transactions.push(Arc::new(tx));
        if let Some(cur) = self.current {
            self.parent.insert(idx, cur);
        }
        self.children.entry(self.current).or_default().push(idx);
        self.current = Some(idx);
        Ok(())
    }

    /// Reverse the current transaction and move to its parent. No-op at
    /// the virtual root.
    ///
    /// # Errors
    /// Propagates failures from [`Transaction::reverse`].
    pub fn undo(&mut self, tree: &mut BlockTree) -> Result<()> {
        let Some(cur) = self.current else {
            return Ok(());
        };
        self.transactions[cur].reverse(tree)?;
        self.current = self.parent.get(&cur).copied();
        Ok(())
    }

    /// Apply the most-recently-added child of the current node. No-op
    /// if the current node has no children.
    ///
    /// # Errors
    /// Propagates failures from [`Transaction::apply`].
    pub fn redo(&mut self, tree: &mut BlockTree) -> Result<()> {
        let Some(&child) = self.children.get(&self.current).and_then(|v| v.last()) else {
            return Ok(());
        };
        self.transactions[child].apply(tree)?;
        self.current = Some(child);
        Ok(())
    }

    /// Move the cursor to `target`, walking via the lowest common
    /// ancestor on the current branch.
    ///
    /// Pass `None` to return to the virtual root.
    ///
    /// # Errors
    /// - [`EditorError::UndoRedo`] if `target` is out of bounds.
    /// - Propagates failures from [`Transaction::apply`] / `reverse`.
    pub fn goto(&mut self, target: Option<usize>, tree: &mut BlockTree) -> Result<()> {
        if target == self.current {
            return Ok(());
        }
        if let Some(t) = target {
            if t >= self.transactions.len() {
                return Err(EditorError::UndoRedo(format!(
                    "goto target index {t} is out of bounds (len {})",
                    self.transactions.len()
                )));
            }
        }

        let current_path = self.path_from_root(self.current);
        let target_path = self.path_from_root(target);

        let mut lca_len = 0;
        while lca_len < current_path.len()
            && lca_len < target_path.len()
            && current_path[lca_len] == target_path[lca_len]
        {
            lca_len += 1;
        }

        // Undo from current back to LCA.
        for &idx in current_path[lca_len..].iter().rev() {
            self.transactions[idx].reverse(tree)?;
        }
        // Apply from LCA forward to target.
        for &idx in &target_path[lca_len..] {
            self.transactions[idx].apply(tree)?;
        }
        self.current = target;
        Ok(())
    }

    /// Children of `idx` (or of the virtual root when `idx` is `None`).
    #[must_use]
    pub fn children_of(&self, idx: Option<usize>) -> &[usize] {
        self.children.get(&idx).map_or(&[], Vec::as_slice)
    }

    // ── Private helpers ──────────────────────────────────────────────────────

    fn path_from_root(&self, idx: Option<usize>) -> Vec<usize> {
        let Some(start) = idx else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut cur = Some(start);
        while let Some(i) = cur {
            out.push(i);
            cur = self.parent.get(&i).copied();
        }
        out.reverse();
        out
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{Block, BlockType, DocumentMetadata};
    use crate::transaction::{Operation, Transaction, TransactionMetadata};

    fn para(text: &str) -> Block {
        Block::new(BlockType::Paragraph).with_content(text)
    }

    fn append_text_tx(block_id: uuid::Uuid, pos: usize, text: &str) -> Transaction {
        Transaction::new(
            vec![Operation::InsertText {
                block_id,
                pos,
                text: text.into(),
                pre_annotations: vec![],
            }],
            TransactionMetadata::default(),
        )
    }

    fn trees_structurally_equal(a: &BlockTree, b: &BlockTree) -> bool {
        a.blocks == b.blocks && a.root_blocks == b.root_blocks
    }

    fn init_tree() -> (BlockTree, uuid::Uuid) {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let id = tree.insert(para(""), None, 0).unwrap();
        (tree, id)
    }

    #[test]
    fn empty_tree_undo_redo_are_noops() {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let mut history = UndoTree::new();
        history.undo(&mut tree).unwrap();
        history.redo(&mut tree).unwrap();
        assert!(history.is_empty());
        assert_eq!(history.current(), None);
    }

    #[test]
    fn linear_undo_redo_back_to_initial_and_final() {
        let (mut tree, id) = init_tree();
        let initial = tree.clone();
        let mut h = UndoTree::new();

        h.execute(append_text_tx(id, 0, "a"), &mut tree).unwrap();
        h.execute(append_text_tx(id, 1, "b"), &mut tree).unwrap();
        h.execute(append_text_tx(id, 2, "c"), &mut tree).unwrap();
        assert_eq!(tree.get(id).unwrap().content, "abc");
        let final_state = tree.clone();

        h.undo(&mut tree).unwrap();
        h.undo(&mut tree).unwrap();
        h.undo(&mut tree).unwrap();
        assert!(trees_structurally_equal(&tree, &initial));
        assert_eq!(h.current(), None);

        h.redo(&mut tree).unwrap();
        h.redo(&mut tree).unwrap();
        h.redo(&mut tree).unwrap();
        assert!(trees_structurally_equal(&tree, &final_state));
    }

    #[test]
    fn execute_after_undo_creates_branch() {
        let (mut tree, id) = init_tree();
        let mut h = UndoTree::new();

        // root → A → B
        h.execute(append_text_tx(id, 0, "A"), &mut tree).unwrap();
        let at_a = tree.clone();
        h.execute(append_text_tx(id, 1, "B"), &mut tree).unwrap();
        let at_b = tree.clone();

        // Back to A, then branch off to C.
        h.undo(&mut tree).unwrap();
        assert!(trees_structurally_equal(&tree, &at_a));
        h.execute(append_text_tx(id, 1, "C"), &mut tree).unwrap();
        assert_eq!(tree.get(id).unwrap().content, "AC");

        // Both children of A still recorded.
        assert_eq!(h.children_of(Some(0)).len(), 2);
        // And both transactions are still in the log (no truncation).
        assert_eq!(h.len(), 3);

        // goto B across the branch.
        h.goto(Some(1), &mut tree).unwrap();
        assert!(trees_structurally_equal(&tree, &at_b));
    }

    #[test]
    fn redo_picks_most_recent_child() {
        let (mut tree, id) = init_tree();
        let mut h = UndoTree::new();
        h.execute(append_text_tx(id, 0, "A"), &mut tree).unwrap();
        h.execute(append_text_tx(id, 1, "B"), &mut tree).unwrap();

        // Back to A.
        h.undo(&mut tree).unwrap();
        // Branch: add C.
        h.execute(append_text_tx(id, 1, "C"), &mut tree).unwrap();
        // Now at C. Undo back to A.
        h.undo(&mut tree).unwrap();

        // Two children of A, most recent first = C.
        h.redo(&mut tree).unwrap();
        assert_eq!(h.current(), Some(2));
        assert_eq!(tree.get(id).unwrap().content, "AC");
    }

    #[test]
    fn goto_none_walks_all_the_way_back() {
        let (mut tree, id) = init_tree();
        let initial = tree.clone();
        let mut h = UndoTree::new();
        h.execute(append_text_tx(id, 0, "A"), &mut tree).unwrap();
        h.execute(append_text_tx(id, 1, "B"), &mut tree).unwrap();

        h.goto(None, &mut tree).unwrap();
        assert!(trees_structurally_equal(&tree, &initial));
        assert_eq!(h.current(), None);
    }

    #[test]
    fn goto_out_of_bounds_errors() {
        let (mut tree, _id) = init_tree();
        let mut h = UndoTree::new();
        assert!(matches!(
            h.goto(Some(42), &mut tree),
            Err(EditorError::UndoRedo(_))
        ));
    }

    #[test]
    fn goto_cross_branch_via_lca() {
        let (mut tree, id) = init_tree();
        let mut h = UndoTree::new();

        // Build: root → A → B → D
        //                 ↳ C → E
        h.execute(append_text_tx(id, 0, "A"), &mut tree).unwrap();
        h.execute(append_text_tx(id, 1, "B"), &mut tree).unwrap();
        let at_b = h.current().unwrap();
        h.execute(append_text_tx(id, 2, "D"), &mut tree).unwrap();

        // Branch off B: go back to B, then C, then E.
        h.goto(Some(at_b), &mut tree).unwrap();
        assert_eq!(tree.get(id).unwrap().content, "AB");
        h.execute(append_text_tx(id, 2, "C"), &mut tree).unwrap();
        h.execute(append_text_tx(id, 3, "E"), &mut tree).unwrap();
        assert_eq!(tree.get(id).unwrap().content, "ABCE");

        // Jump across to D (sibling branch). LCA is B.
        let d_idx = 2; // third executed tx
        h.goto(Some(d_idx), &mut tree).unwrap();
        assert_eq!(tree.get(id).unwrap().content, "ABD");
        assert_eq!(h.current(), Some(d_idx));
    }

    #[test]
    fn goto_between_different_virtual_root_branches() {
        let (mut tree, id) = init_tree();
        let mut h = UndoTree::new();
        h.execute(append_text_tx(id, 0, "A"), &mut tree).unwrap();
        // Back to root, then an unrelated branch.
        h.goto(None, &mut tree).unwrap();
        h.execute(append_text_tx(id, 0, "B"), &mut tree).unwrap();
        // Now two direct children of the virtual root: [0, 1].
        assert_eq!(h.children_of(None).len(), 2);

        // Hop back to the A branch (tx 0).
        h.goto(Some(0), &mut tree).unwrap();
        assert_eq!(tree.get(id).unwrap().content, "A");
    }

    #[test]
    fn execute_failure_leaves_history_untouched() {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let mut h = UndoTree::new();

        // Reference a block that doesn't exist.
        let tx = append_text_tx(uuid::Uuid::new_v4(), 0, "x");
        let err = h.execute(tx, &mut tree);
        assert!(err.is_err());
        assert!(h.is_empty());
        assert_eq!(h.current(), None);
    }
}
