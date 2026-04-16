//! The block tree data structure and navigation helpers (PRD 08 §1.3).
//!
//! A [`BlockTree`] owns every [`Block`] in a single document, together
//! with the ordered list of root blocks and document-level metadata.
//!
//! # Invariants
//!
//! - Every non-root block has a `parent_id` that resolves to a block
//!   in `blocks`, and the parent's `children` vector contains this
//!   block's id at position `index_in_parent`.
//! - Every root block has `parent_id == None` and appears exactly once
//!   in `root_blocks`.
//! - `index_in_parent` is consistent with the slot within the parent's
//!   `children` vector (or within `root_blocks`).
//!
//! [`BlockTree::validate`] checks all of these and is called from debug
//! assertions in the transaction layer.

use std::collections::{HashMap, HashSet};

use crate::block::{Block, BlockId, DocumentMetadata};
use crate::error::{EditorError, Result};

/// Ordered, indexed tree of blocks plus document metadata.
#[derive(Clone, Debug, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct BlockTree {
    /// All blocks in the document, indexed by id.
    pub blocks: HashMap<BlockId, Block>,
    /// Ordered list of root block ids.
    pub root_blocks: Vec<BlockId>,
    /// Document-level metadata.
    pub metadata: DocumentMetadata,
}

impl BlockTree {
    /// Create an empty tree with the given metadata.
    #[must_use]
    pub fn new(metadata: DocumentMetadata) -> Self {
        Self {
            blocks: HashMap::new(),
            root_blocks: Vec::new(),
            metadata,
        }
    }

    // ── Accessors ────────────────────────────────────────────────────────────

    /// Borrow a block by id.
    #[must_use]
    pub fn get(&self, id: BlockId) -> Option<&Block> {
        self.blocks.get(&id)
    }

    /// Mutable block accessor.
    pub fn get_mut(&mut self, id: BlockId) -> Option<&mut Block> {
        self.blocks.get_mut(&id)
    }

    /// `true` if the tree contains no blocks.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    // ── Navigation (PRD 1.3) ─────────────────────────────────────────────────

    /// The parent block, or `None` for roots.
    #[must_use]
    pub fn parent(&self, block_id: BlockId) -> Option<&Block> {
        let pid = self.blocks.get(&block_id)?.parent_id?;
        self.blocks.get(&pid)
    }

    /// Ordered child blocks.
    #[must_use]
    pub fn children(&self, block_id: BlockId) -> Vec<&Block> {
        self.blocks
            .get(&block_id)
            .map(|b| {
                b.children
                    .iter()
                    .filter_map(|cid| self.blocks.get(cid))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Next sibling within the parent's children (or root list).
    #[must_use]
    pub fn next_sibling(&self, block_id: BlockId) -> Option<&Block> {
        let block = self.blocks.get(&block_id)?;
        let siblings = self.sibling_list(block);
        siblings
            .get(block.index_in_parent + 1)
            .and_then(|id| self.blocks.get(id))
    }

    /// Previous sibling within the parent's children (or root list).
    #[must_use]
    pub fn prev_sibling(&self, block_id: BlockId) -> Option<&Block> {
        let block = self.blocks.get(&block_id)?;
        if block.index_in_parent == 0 {
            return None;
        }
        let siblings = self.sibling_list(block);
        siblings
            .get(block.index_in_parent - 1)
            .and_then(|id| self.blocks.get(id))
    }

    /// All descendant ids in document order (pre-order).
    #[must_use]
    pub fn descendants(&self, block_id: BlockId) -> Vec<BlockId> {
        let mut out = Vec::new();
        self.descend(block_id, &mut out);
        out
    }

    fn descend(&self, block_id: BlockId, out: &mut Vec<BlockId>) {
        let Some(block) = self.blocks.get(&block_id) else {
            return;
        };
        for &cid in &block.children {
            out.push(cid);
            self.descend(cid, out);
        }
    }

    fn sibling_list(&self, block: &Block) -> &[BlockId] {
        if let Some(pid) = block.parent_id {
            self.blocks
                .get(&pid)
                .map_or(&[] as &[BlockId], |p| p.children.as_slice())
        } else {
            self.root_blocks.as_slice()
        }
    }

    // ── Mutation (used by the transaction layer) ─────────────────────────────

    /// Insert `block` under `parent` at the given `index`.
    ///
    /// # Errors
    /// - [`EditorError::InvalidTree`] if `parent` doesn't exist,
    ///   `index` is out of bounds, or `block.id` already exists.
    pub fn insert(
        &mut self,
        mut block: Block,
        parent: Option<BlockId>,
        index: usize,
    ) -> Result<BlockId> {
        if self.blocks.contains_key(&block.id) {
            return Err(EditorError::InvalidTree(format!(
                "duplicate block id: {}",
                block.id
            )));
        }
        let id = block.id;
        block.parent_id = parent;
        block.index_in_parent = index;

        if let Some(pid) = parent {
            let parent_block = self
                .blocks
                .get_mut(&pid)
                .ok_or(EditorError::BlockNotFound(pid))?;
            if index > parent_block.children.len() {
                return Err(EditorError::InvalidTree(format!(
                    "insert index {index} out of bounds ({} children)",
                    parent_block.children.len()
                )));
            }
            parent_block.children.insert(index, id);
            let following: Vec<BlockId> = parent_block
                .children
                .iter()
                .skip(index + 1)
                .copied()
                .collect();
            for (offset, sid) in following.into_iter().enumerate() {
                if let Some(sibling) = self.blocks.get_mut(&sid) {
                    sibling.index_in_parent = index + 1 + offset;
                }
            }
        } else {
            if index > self.root_blocks.len() {
                return Err(EditorError::InvalidTree(format!(
                    "insert index {index} out of bounds ({} roots)",
                    self.root_blocks.len()
                )));
            }
            self.root_blocks.insert(index, id);
            for (i, sid) in self.root_blocks.iter().enumerate().skip(index + 1) {
                let sid = *sid;
                if let Some(sibling) = self.blocks.get_mut(&sid) {
                    sibling.index_in_parent = i;
                }
            }
        }

        self.blocks.insert(id, block);
        Ok(id)
    }

    /// Remove a leaf block and return it.
    ///
    /// The block must have no children. To delete a subtree, remove
    /// descendants first (post-order).
    ///
    /// # Errors
    /// - [`EditorError::BlockNotFound`] if `id` isn't present.
    /// - [`EditorError::InvalidTree`] if `id` still has children.
    pub fn remove(&mut self, id: BlockId) -> Result<Block> {
        let block = self.blocks.get(&id).ok_or(EditorError::BlockNotFound(id))?;
        if !block.children.is_empty() {
            return Err(EditorError::InvalidTree(format!(
                "cannot remove non-leaf block {id} ({} children remain)",
                block.children.len()
            )));
        }
        let parent_id = block.parent_id;
        let index = block.index_in_parent;

        if let Some(pid) = parent_id {
            let parent_block = self
                .blocks
                .get_mut(&pid)
                .ok_or_else(|| EditorError::InvalidTree(format!("parent {pid} of {id} missing")))?;
            if parent_block.children.get(index) != Some(&id) {
                return Err(EditorError::InvalidTree(format!(
                    "block {id} not at expected index {index} of parent {pid}"
                )));
            }
            parent_block.children.remove(index);
            let following: Vec<BlockId> =
                parent_block.children.iter().skip(index).copied().collect();
            for (offset, sid) in following.into_iter().enumerate() {
                if let Some(sibling) = self.blocks.get_mut(&sid) {
                    sibling.index_in_parent = index + offset;
                }
            }
        } else {
            if self.root_blocks.get(index) != Some(&id) {
                return Err(EditorError::InvalidTree(format!(
                    "root block {id} not at expected index {index}"
                )));
            }
            self.root_blocks.remove(index);
            for (i, sid) in self.root_blocks.iter().enumerate().skip(index) {
                let sid = *sid;
                if let Some(sibling) = self.blocks.get_mut(&sid) {
                    sibling.index_in_parent = i;
                }
            }
        }

        let mut removed = self
            .blocks
            .remove(&id)
            .ok_or(EditorError::BlockNotFound(id))?;
        removed.parent_id = parent_id;
        removed.index_in_parent = index;
        Ok(removed)
    }

    /// Move `id` (and its subtree) to become a child of `new_parent`
    /// at `new_index`.
    ///
    /// `new_index` is interpreted in the **target** sibling list as it
    /// looked before the block was detached — reparenting forward
    /// within the same list adjusts automatically.
    ///
    /// # Errors
    /// - [`EditorError::BlockNotFound`] if `id` or `new_parent` is missing.
    /// - [`EditorError::InvalidTree`] if `new_parent` is `id` itself or
    ///   a descendant, or if indices are out of range.
    pub fn reparent(
        &mut self,
        id: BlockId,
        new_parent: Option<BlockId>,
        new_index: usize,
    ) -> Result<()> {
        let (old_parent, old_index) = {
            let b = self.blocks.get(&id).ok_or(EditorError::BlockNotFound(id))?;
            (b.parent_id, b.index_in_parent)
        };
        if let Some(np) = new_parent {
            if np == id {
                return Err(EditorError::InvalidTree(format!(
                    "cannot reparent block {id} under itself"
                )));
            }
            if !self.blocks.contains_key(&np) {
                return Err(EditorError::BlockNotFound(np));
            }
            if self.descendants(id).contains(&np) {
                return Err(EditorError::InvalidTree(format!(
                    "cannot reparent {id} under its descendant {np}"
                )));
            }
        }

        self.unlink_from_siblings(id, old_parent, old_index)?;

        let adjusted_new_index = if old_parent == new_parent && new_index > old_index {
            new_index - 1
        } else {
            new_index
        };
        self.link_into_siblings(id, new_parent, adjusted_new_index)?;
        if let Some(block) = self.blocks.get_mut(&id) {
            block.parent_id = new_parent;
            block.index_in_parent = adjusted_new_index;
        }
        Ok(())
    }

    fn unlink_from_siblings(
        &mut self,
        id: BlockId,
        parent: Option<BlockId>,
        index: usize,
    ) -> Result<()> {
        if let Some(pid) = parent {
            let p = self
                .blocks
                .get_mut(&pid)
                .ok_or_else(|| EditorError::InvalidTree(format!("parent {pid} of {id} missing")))?;
            if p.children.get(index) != Some(&id) {
                return Err(EditorError::InvalidTree(format!(
                    "block {id} not at expected index {index} of parent {pid}"
                )));
            }
            p.children.remove(index);
            let following: Vec<BlockId> = p.children.iter().skip(index).copied().collect();
            for (offset, sid) in following.into_iter().enumerate() {
                if let Some(s) = self.blocks.get_mut(&sid) {
                    s.index_in_parent = index + offset;
                }
            }
        } else {
            if self.root_blocks.get(index) != Some(&id) {
                return Err(EditorError::InvalidTree(format!(
                    "root block {id} not at expected index {index}"
                )));
            }
            self.root_blocks.remove(index);
            for (i, sid) in self.root_blocks.iter().enumerate().skip(index) {
                let sid = *sid;
                if let Some(s) = self.blocks.get_mut(&sid) {
                    s.index_in_parent = i;
                }
            }
        }
        Ok(())
    }

    fn link_into_siblings(
        &mut self,
        id: BlockId,
        parent: Option<BlockId>,
        index: usize,
    ) -> Result<()> {
        if let Some(pid) = parent {
            let p = self
                .blocks
                .get_mut(&pid)
                .ok_or(EditorError::BlockNotFound(pid))?;
            if index > p.children.len() {
                return Err(EditorError::InvalidTree(format!(
                    "link index {index} out of bounds ({} children)",
                    p.children.len()
                )));
            }
            p.children.insert(index, id);
            let following: Vec<BlockId> = p.children.iter().skip(index + 1).copied().collect();
            for (offset, sid) in following.into_iter().enumerate() {
                if let Some(s) = self.blocks.get_mut(&sid) {
                    s.index_in_parent = index + 1 + offset;
                }
            }
        } else {
            if index > self.root_blocks.len() {
                return Err(EditorError::InvalidTree(format!(
                    "link index {index} out of bounds ({} roots)",
                    self.root_blocks.len()
                )));
            }
            self.root_blocks.insert(index, id);
            for (i, sid) in self.root_blocks.iter().enumerate().skip(index + 1) {
                let sid = *sid;
                if let Some(s) = self.blocks.get_mut(&sid) {
                    s.index_in_parent = i;
                }
            }
        }
        Ok(())
    }

    // ── Validation ───────────────────────────────────────────────────────────

    /// Check every invariant described on the struct.
    ///
    /// # Errors
    /// - [`EditorError::InvalidTree`] with a description of the first
    ///   violation found.
    pub fn validate(&self) -> Result<()> {
        let mut seen_as_child: HashSet<BlockId> = HashSet::new();

        for (i, id) in self.root_blocks.iter().enumerate() {
            let b = self.blocks.get(id).ok_or_else(|| {
                EditorError::InvalidTree(format!("root {id} missing from blocks"))
            })?;
            if b.parent_id.is_some() {
                return Err(EditorError::InvalidTree(format!(
                    "root {id} has parent_id {:?}",
                    b.parent_id
                )));
            }
            if b.index_in_parent != i {
                return Err(EditorError::InvalidTree(format!(
                    "root {id} index_in_parent {} != root slot {i}",
                    b.index_in_parent
                )));
            }
        }

        for (id, block) in &self.blocks {
            for (i, &cid) in block.children.iter().enumerate() {
                let child = self.blocks.get(&cid).ok_or_else(|| {
                    EditorError::InvalidTree(format!("child {cid} of {id} missing from blocks"))
                })?;
                if child.parent_id != Some(*id) {
                    return Err(EditorError::InvalidTree(format!(
                        "child {cid} claims parent {:?}, expected {id}",
                        child.parent_id
                    )));
                }
                if child.index_in_parent != i {
                    return Err(EditorError::InvalidTree(format!(
                        "child {cid} index_in_parent {} != parent slot {i}",
                        child.index_in_parent
                    )));
                }
                if !seen_as_child.insert(cid) {
                    return Err(EditorError::InvalidTree(format!(
                        "child {cid} appears under multiple parents"
                    )));
                }
            }
        }

        // Every non-root block must appear as someone's child.
        for (id, block) in &self.blocks {
            if block.parent_id.is_none() && !self.root_blocks.contains(id) {
                return Err(EditorError::InvalidTree(format!(
                    "orphan block {id} has no parent and is not a root"
                )));
            }
        }

        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{Block, BlockType};

    fn para(text: &str) -> Block {
        Block::new(BlockType::Paragraph).with_content(text)
    }

    #[test]
    fn empty_tree_accessors() {
        let t = BlockTree::default();
        assert!(t.is_empty());
        assert!(t.root_blocks.is_empty());
        assert_eq!(t.children(uuid::Uuid::nil()), Vec::<&Block>::new());
        assert!(t.descendants(uuid::Uuid::nil()).is_empty());
    }

    #[test]
    fn insert_root_updates_root_blocks() {
        let mut t = BlockTree::default();
        let a = t.insert(para("a"), None, 0).unwrap();
        assert_eq!(t.root_blocks, vec![a]);
        assert_eq!(t.get(a).unwrap().index_in_parent, 0);
        t.validate().unwrap();
    }

    #[test]
    fn insert_reindexes_root_siblings() {
        let mut t = BlockTree::default();
        let a = t.insert(para("a"), None, 0).unwrap();
        let b = t.insert(para("b"), None, 1).unwrap();
        let c = t.insert(para("c"), None, 1).unwrap(); // between a and b
        assert_eq!(t.root_blocks, vec![a, c, b]);
        assert_eq!(t.get(a).unwrap().index_in_parent, 0);
        assert_eq!(t.get(c).unwrap().index_in_parent, 1);
        assert_eq!(t.get(b).unwrap().index_in_parent, 2);
        t.validate().unwrap();
    }

    #[test]
    fn insert_under_parent() {
        let mut t = BlockTree::default();
        let root = t.insert(para("r"), None, 0).unwrap();
        let child = t.insert(para("c"), Some(root), 0).unwrap();
        assert_eq!(t.get(child).unwrap().parent_id, Some(root));
        assert_eq!(t.get(root).unwrap().children, vec![child]);
        t.validate().unwrap();
    }

    #[test]
    fn insert_duplicate_id_errors() {
        let mut t = BlockTree::default();
        let b = para("a");
        let id = b.id;
        t.insert(b.clone(), None, 0).unwrap();
        let mut dup = para("dup");
        dup.id = id;
        assert!(matches!(
            t.insert(dup, None, 0),
            Err(EditorError::InvalidTree(_))
        ));
    }

    #[test]
    fn insert_out_of_bounds_errors() {
        let mut t = BlockTree::default();
        t.insert(para("a"), None, 0).unwrap();
        let r = t.insert(para("b"), None, 99);
        assert!(matches!(r, Err(EditorError::InvalidTree(_))));
    }

    #[test]
    fn navigation_parent_children_siblings() {
        let mut t = BlockTree::default();
        let root = t.insert(para("r"), None, 0).unwrap();
        let a = t.insert(para("a"), Some(root), 0).unwrap();
        let b = t.insert(para("b"), Some(root), 1).unwrap();
        let c = t.insert(para("c"), Some(root), 2).unwrap();

        assert_eq!(t.parent(a).unwrap().id, root);
        let kids: Vec<_> = t.children(root).iter().map(|b| b.id).collect();
        assert_eq!(kids, vec![a, b, c]);
        assert_eq!(t.next_sibling(a).unwrap().id, b);
        assert_eq!(t.prev_sibling(b).unwrap().id, a);
        assert!(t.next_sibling(c).is_none());
        assert!(t.prev_sibling(a).is_none());
    }

    #[test]
    fn descendants_pre_order() {
        let mut t = BlockTree::default();
        let root = t.insert(para("r"), None, 0).unwrap();
        let a = t.insert(para("a"), Some(root), 0).unwrap();
        let a1 = t.insert(para("a1"), Some(a), 0).unwrap();
        let a2 = t.insert(para("a2"), Some(a), 1).unwrap();
        let b = t.insert(para("b"), Some(root), 1).unwrap();
        assert_eq!(t.descendants(root), vec![a, a1, a2, b]);
    }

    #[test]
    fn remove_leaf_updates_parent_and_reindexes_siblings() {
        let mut t = BlockTree::default();
        let root = t.insert(para("r"), None, 0).unwrap();
        let a = t.insert(para("a"), Some(root), 0).unwrap();
        let b = t.insert(para("b"), Some(root), 1).unwrap();
        let c = t.insert(para("c"), Some(root), 2).unwrap();

        let removed = t.remove(b).unwrap();
        assert_eq!(removed.id, b);
        assert_eq!(t.get(root).unwrap().children, vec![a, c]);
        assert_eq!(t.get(a).unwrap().index_in_parent, 0);
        assert_eq!(t.get(c).unwrap().index_in_parent, 1);
        assert!(t.get(b).is_none());
        t.validate().unwrap();
    }

    #[test]
    fn remove_non_leaf_errors() {
        let mut t = BlockTree::default();
        let root = t.insert(para("r"), None, 0).unwrap();
        let _ = t.insert(para("c"), Some(root), 0).unwrap();
        assert!(matches!(t.remove(root), Err(EditorError::InvalidTree(_))));
    }

    #[test]
    fn remove_root_leaf_updates_root_blocks() {
        let mut t = BlockTree::default();
        let a = t.insert(para("a"), None, 0).unwrap();
        let b = t.insert(para("b"), None, 1).unwrap();
        t.remove(a).unwrap();
        assert_eq!(t.root_blocks, vec![b]);
        assert_eq!(t.get(b).unwrap().index_in_parent, 0);
        t.validate().unwrap();
    }

    #[test]
    fn validate_detects_mismatched_parent_id() {
        let mut t = BlockTree::default();
        let root = t.insert(para("r"), None, 0).unwrap();
        let child = t.insert(para("c"), Some(root), 0).unwrap();
        // Corrupt parent pointer.
        t.blocks.get_mut(&child).unwrap().parent_id = None;
        assert!(matches!(t.validate(), Err(EditorError::InvalidTree(_))));
    }

    #[test]
    fn reparent_moves_block_between_parents() {
        let mut t = BlockTree::default();
        let p1 = t.insert(para("p1"), None, 0).unwrap();
        let p2 = t.insert(para("p2"), None, 1).unwrap();
        let a = t.insert(para("a"), Some(p1), 0).unwrap();
        let b = t.insert(para("b"), Some(p2), 0).unwrap();

        t.reparent(a, Some(p2), 1).unwrap();
        assert!(t.get(p1).unwrap().children.is_empty());
        assert_eq!(t.get(p2).unwrap().children, vec![b, a]);
        assert_eq!(t.get(a).unwrap().parent_id, Some(p2));
        assert_eq!(t.get(a).unwrap().index_in_parent, 1);
        t.validate().unwrap();
    }

    #[test]
    fn reparent_within_same_parent_adjusts_index() {
        let mut t = BlockTree::default();
        let root = t.insert(para("r"), None, 0).unwrap();
        let a = t.insert(para("a"), Some(root), 0).unwrap();
        let b = t.insert(para("b"), Some(root), 1).unwrap();
        let c = t.insert(para("c"), Some(root), 2).unwrap();

        // Move `a` (currently idx 0) to end (target idx 3 in pre-detach frame)
        t.reparent(a, Some(root), 3).unwrap();
        assert_eq!(t.get(root).unwrap().children, vec![b, c, a]);
        t.validate().unwrap();
    }

    #[test]
    fn reparent_under_descendant_errors() {
        let mut t = BlockTree::default();
        let root = t.insert(para("r"), None, 0).unwrap();
        let mid = t.insert(para("m"), Some(root), 0).unwrap();
        assert!(matches!(
            t.reparent(root, Some(mid), 0),
            Err(EditorError::InvalidTree(_))
        ));
    }

    #[test]
    fn validate_detects_mismatched_index_in_parent() {
        let mut t = BlockTree::default();
        let a = t.insert(para("a"), None, 0).unwrap();
        t.blocks.get_mut(&a).unwrap().index_in_parent = 99;
        assert!(matches!(t.validate(), Err(EditorError::InvalidTree(_))));
    }
}
