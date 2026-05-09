//! RGA-style sequence CRDT for in-block character-level text merge.
//!
//! Used by Phase 2 of [`crate::CrdtDoc`] to resolve concurrent text
//! edits within the same block silently. Phase 1 keeps it standalone
//! (with its own tests) so the algorithm can be exercised and reviewed
//! before it becomes load-bearing for [`crate::CrdtDoc`].
//!
//! # Algorithm
//!
//! Each visible character is a node with a unique [`OpId`] and an
//! optional parent (the character to its left at the moment of
//! authoring; `None` for the document head). Sibling nodes — those
//! sharing a parent — are ordered by **descending** `OpId`, which is
//! the standard RGA tiebreak: newer authors win to the left of older
//! authors.
//!
//! Deletion is a tombstone: the node stays in the tree (so future
//! insertions that referenced it as parent still resolve) but contributes
//! nothing to the rendered string.
//!
//! Idempotency: applying the same [`RgaTextOp`] twice is a no-op. This
//! is what makes the layer safe to drive from a gossip pipeline.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::id::OpId;

/// An RGA primitive — the wire format for a single-character edit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RgaTextOp {
    /// Insert `ch` after `parent` (or at the head when `parent` is
    /// `None`). The new character takes [`OpId`] `id`.
    Insert {
        /// Op id of the new character.
        id: OpId,
        /// Op id of the character this one was authored to the right
        /// of, or `None` to anchor at the document head.
        parent: Option<OpId>,
        /// The character itself.
        ch: char,
    },
    /// Tombstone the character with op id `target`.
    Delete {
        /// The new op id authoring this delete (used as `vv` cursor
        /// at the doc level — the RGA itself doesn't need it).
        id: OpId,
        /// Character to remove.
        target: OpId,
    },
}

impl RgaTextOp {
    /// The op id authoring this primitive (for vv accounting).
    #[must_use]
    pub fn id(&self) -> OpId {
        match self {
            Self::Insert { id, .. } | Self::Delete { id, .. } => *id,
        }
    }
}

#[derive(Clone, Debug)]
struct Node {
    ch: char,
    tombstone: bool,
    /// Direct children, sorted **descending** by `OpId` (the RGA order).
    children: Vec<OpId>,
}

/// In-memory state for the RGA sequence CRDT for a single piece of
/// text (e.g., one block's `content`).
#[derive(Clone, Debug, Default)]
pub struct RgaText {
    nodes: HashMap<OpId, Node>,
    /// Top-level nodes (parent == None), sorted descending by `OpId`.
    roots: Vec<OpId>,
}

impl RgaText {
    /// Empty text.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Length in **visible** characters (tombstones excluded). O(n).
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.values().filter(|n| !n.tombstone).count()
    }

    /// True if the text has no visible characters.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Render to a `String` in RGA traversal order.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        for &root in &self.roots {
            self.visit(root, &mut out);
        }
        out
    }

    fn visit(&self, id: OpId, out: &mut String) {
        let Some(node) = self.nodes.get(&id) else {
            return;
        };
        if !node.tombstone {
            out.push(node.ch);
        }
        for &child in &node.children {
            self.visit(child, out);
        }
    }

    /// Build the **wire** op for a local insertion at the given
    /// visible character position. Pass the freshly-allocated [`OpId`]
    /// as `id`. The caller is responsible for then calling
    /// [`Self::apply`] (or letting it round-trip via the doc).
    ///
    /// `pos == 0` inserts at the document head; `pos == len()` inserts
    /// at the tail.
    #[must_use]
    pub fn build_insert(&self, pos: usize, ch: char, id: OpId) -> RgaTextOp {
        let parent = self.id_at_visible_index(pos.checked_sub(1));
        RgaTextOp::Insert { id, parent, ch }
    }

    /// Build the wire op for a local deletion at the given visible
    /// character position. Returns `None` if `pos` is out of range or
    /// the character at `pos` is already tombstoned.
    #[must_use]
    pub fn build_delete(&self, pos: usize, id: OpId) -> Option<RgaTextOp> {
        let target = self.id_at_visible_index(Some(pos))?;
        Some(RgaTextOp::Delete { id, target })
    }

    /// Build an [`RgaText`] representing a baseline string of
    /// characters, where each character's [`OpId`] is provided by
    /// `make_id`. Successive characters are chained via parent linkage
    /// (`make_id(0)` parents `make_id(1)`, etc.) so future inserts can
    /// anchor at any visible position.
    ///
    /// Used by [`crate::CrdtDoc`] to materialise a per-block RGA mirror
    /// from the baseline tree content. Both peers feed the same
    /// `make_id` (see [`crate::merge::baseline_op_id`]), so they end up
    /// with identical RGA state.
    #[must_use]
    pub fn from_chars<I, F>(chars: I, make_id: F) -> Self
    where
        I: IntoIterator<Item = char>,
        F: Fn(usize) -> OpId,
    {
        let mut text = Self::new();
        let mut prev: Option<OpId> = None;
        for (i, ch) in chars.into_iter().enumerate() {
            let id = make_id(i);
            let op = RgaTextOp::Insert {
                id,
                parent: prev,
                ch,
            };
            text.apply(&op);
            prev = Some(id);
        }
        text
    }

    /// [`OpId`] of the character at the given visible-character index,
    /// or `None` if the index is out of range.
    #[must_use]
    pub fn op_id_at(&self, pos: usize) -> Option<OpId> {
        self.id_at_visible_index(Some(pos))
    }

    /// Apply a wire op. Idempotent. Returns `true` iff the state
    /// changed.
    pub fn apply(&mut self, op: &RgaTextOp) -> bool {
        match op {
            RgaTextOp::Insert { id, parent, ch } => self.apply_insert(*id, *parent, *ch),
            RgaTextOp::Delete { target, .. } => self.apply_delete(*target),
        }
    }

    fn apply_insert(&mut self, id: OpId, parent: Option<OpId>, ch: char) -> bool {
        if self.nodes.contains_key(&id) {
            return false;
        }
        if let Some(p) = parent {
            if !self.nodes.contains_key(&p) {
                // Phase 1 expects causal delivery from the doc layer.
                // In the standalone tests we only exercise causally-
                // ordered streams.
                return false;
            }
        }
        self.nodes.insert(
            id,
            Node {
                ch,
                tombstone: false,
                children: vec![],
            },
        );
        let siblings = match parent {
            None => &mut self.roots,
            Some(p) => &mut self
                .nodes
                .get_mut(&p)
                .expect("parent presence checked above")
                .children,
        };
        let pos = siblings
            .iter()
            .position(|sib| *sib < id)
            .unwrap_or(siblings.len());
        siblings.insert(pos, id);
        true
    }

    fn apply_delete(&mut self, target: OpId) -> bool {
        let Some(node) = self.nodes.get_mut(&target) else {
            return false;
        };
        if node.tombstone {
            return false;
        }
        node.tombstone = true;
        true
    }

    /// Walk RGA order, skipping tombstones, and return the [`OpId`] at
    /// the given visible index. `None` selects the head sentinel
    /// (i.e. `id_at_visible_index(None) == None`).
    fn id_at_visible_index(&self, pos: Option<usize>) -> Option<OpId> {
        let target = pos?;
        let mut counter: usize = 0;
        let mut found = None;
        let mut stack: Vec<OpId> = self.roots.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            if found.is_some() {
                break;
            }
            let Some(node) = self.nodes.get(&id) else {
                continue;
            };
            if !node.tombstone {
                if counter == target {
                    found = Some(id);
                }
                counter = counter.saturating_add(1);
            }
            for &child in node.children.iter().rev() {
                stack.push(child);
            }
        }
        found
    }
}

#[cfg(test)]
#[allow(clippy::many_single_char_names)] // Test fixtures favour `a`/`b`/`c` for the canonical "abc" string scenarios.
mod tests {
    use super::*;
    use crate::id::{Lamport, SiteId};

    fn id(site: SiteId, l: u64) -> OpId {
        OpId::new(site, Lamport(l))
    }

    /// Helper: insert a char at end-of-text on the given site.
    fn append(text: &mut RgaText, site: SiteId, lamport: u64, ch: char) -> RgaTextOp {
        let op = text.build_insert(text.len(), ch, id(site, lamport));
        text.apply(&op);
        op
    }

    #[test]
    fn linear_insertion_renders_in_order() {
        let s = SiteId::new();
        let mut t = RgaText::new();
        append(&mut t, s, 1, 'a');
        append(&mut t, s, 2, 'b');
        append(&mut t, s, 3, 'c');
        assert_eq!(t.render(), "abc");
        assert_eq!(t.len(), 3);
    }

    #[test]
    fn insertions_at_head_compose() {
        let s = SiteId::new();
        let mut t = RgaText::new();
        let a = t.build_insert(0, 'a', id(s, 1));
        t.apply(&a);
        let b = t.build_insert(0, 'b', id(s, 2));
        t.apply(&b);
        let c = t.build_insert(0, 'c', id(s, 3));
        t.apply(&c);
        // Each insert at pos 0 anchors at head; newer (higher OpId)
        // comes leftmost among siblings of None.
        assert_eq!(t.render(), "cba");
    }

    #[test]
    fn delete_tombstones_the_character() {
        let s = SiteId::new();
        let mut t = RgaText::new();
        append(&mut t, s, 1, 'a');
        append(&mut t, s, 2, 'b');
        append(&mut t, s, 3, 'c');
        let d = t.build_delete(1, id(s, 4)).expect("middle char exists");
        t.apply(&d);
        assert_eq!(t.render(), "ac");
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn apply_is_idempotent() {
        let s = SiteId::new();
        let mut t = RgaText::new();
        let op = t.build_insert(0, 'x', id(s, 1));
        assert!(t.apply(&op));
        assert!(!t.apply(&op), "second apply must be a no-op");
        assert_eq!(t.render(), "x");

        let del = t.build_delete(0, id(s, 2)).unwrap();
        assert!(t.apply(&del));
        assert!(!t.apply(&del), "second delete must be a no-op");
    }

    #[test]
    fn concurrent_inserts_at_head_converge() {
        let s1 = SiteId::new();
        let s2 = SiteId::new();
        // Both sites start from an empty doc, both insert at pos 0
        // with the same Lamport. Site-id breaks the tie.
        let op1 = RgaTextOp::Insert {
            id: id(s1, 1),
            parent: None,
            ch: 'X',
        };
        let op2 = RgaTextOp::Insert {
            id: id(s2, 1),
            parent: None,
            ch: 'Y',
        };

        let mut a = RgaText::new();
        a.apply(&op1);
        a.apply(&op2);
        let mut b = RgaText::new();
        b.apply(&op2);
        b.apply(&op1);

        assert_eq!(a.render(), b.render(), "RGA must converge");
        // Newer (higher) OpId leftmost. Whichever site has the larger
        // UUID wins the leftmost position.
        let winner = if id(s1, 1) > id(s2, 1) { 'X' } else { 'Y' };
        assert!(a.render().starts_with(winner));
    }

    #[test]
    fn three_site_interleaving_converges() {
        let s1 = SiteId::new();
        let s2 = SiteId::new();
        let s3 = SiteId::new();

        // Author "abc" at site 1.
        let mut canonical = RgaText::new();
        let a = canonical.build_insert(0, 'a', id(s1, 1));
        canonical.apply(&a);
        let b = canonical.build_insert(1, 'b', id(s1, 2));
        canonical.apply(&b);
        let c = canonical.build_insert(2, 'c', id(s1, 3));
        canonical.apply(&c);

        // Site 2 inserts 'X' between a and b (concurrent with site 3).
        let s2_op = RgaTextOp::Insert {
            id: id(s2, 4),
            parent: Some(a.id()),
            ch: 'X',
        };
        // Site 3 also inserts 'Y' between a and b.
        let s3_op = RgaTextOp::Insert {
            id: id(s3, 4),
            parent: Some(a.id()),
            ch: 'Y',
        };

        // Replay in different orders.
        let ops = [a.clone(), b.clone(), c.clone(), s2_op.clone(), s3_op.clone()];
        let permutations = [
            [0, 1, 2, 3, 4],
            [0, 1, 2, 4, 3],
            [0, 3, 1, 4, 2],
            [0, 4, 1, 3, 2],
        ];
        let mut renders = vec![];
        for order in permutations {
            let mut t = RgaText::new();
            for i in order {
                t.apply(&ops[i]);
            }
            renders.push(t.render());
        }
        assert!(
            renders.windows(2).all(|w| w[0] == w[1]),
            "all replays must converge, got {renders:?}"
        );
    }
}

