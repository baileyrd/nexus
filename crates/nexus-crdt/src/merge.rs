//! Phase 2 helpers: deterministic synthetic [`OpId`] derivation and the
//! translation from byte-positional [`Operation`]s to position-free
//! [`crate::text::RgaTextOp`] sequences.
//!
//! ## Why synthetic ids
//!
//! A new [`crate::CrdtDoc`] starts from a [`BlockTree`] whose content
//! existed before any CRDT op was authored. The Phase 2 silent-merge
//! pipeline keeps a per-block [`RgaText`] mirror eagerly so concurrent
//! `InsertText`/`DeleteText` can converge without surfacing a conflict.
//! That mirror needs *some* [`OpId`] for every baseline character so
//! later inserts can anchor against them.
//!
//! [`baseline_op_id`] is the deterministic seed: every site that
//! constructs a `CrdtDoc` from the same baseline tree materialises the
//! identical synthetic chain, so when ops gossip across, both sites
//! share a single coherent RGA tree.
//!
//! ## Sub-op ids
//!
//! A multi-character `InsertText { text: "abc" }` translates to three
//! `RgaTextOp::Insert` chained as `a → b → c`. The envelope [`CrdtOp`]
//! carries one [`OpId`]; [`subop_id`] derives the per-character ids
//! deterministically so two sites authoring the same multi-char insert
//! see the same chain ids on both ends.

use nexus_editor::BlockId;
use uuid::Uuid;

use crate::id::{Lamport, OpId, SiteId};

/// Namespace for baseline-character synthetic [`OpId`]s. Random 16-byte
/// constant; only its uniqueness vs other namespaces matters.
const BASELINE_NS: Uuid = Uuid::from_bytes([
    0xb1, 0x07, 0x4c, 0x07, 0xba, 0x5e, 0x11, 0x70, 0x9a, 0x07, 0xc0, 0x09, 0x73, 0x12, 0x57, 0x10,
]);

/// Namespace for per-character sub-op ids inside a multi-char insert.
const SUBOP_NS: Uuid = Uuid::from_bytes([
    0xb1, 0x07, 0x4c, 0x07, 0xc4, 0xa1, 0x5b, 0x70, 0x9a, 0x07, 0xc0, 0x09, 0x73, 0x12, 0x57, 0x11,
]);

/// Deterministic synthetic [`OpId`] for the `char_pos`-th character of
/// the *baseline* content of `block_id`.
///
/// Lamport is fixed at `0` so any real op (lamport ≥ 1) sorts after
/// these in [`OpId`] order — essential for the RGA tiebreak that keeps
/// new edits leftward of older content within a sibling group.
#[must_use]
pub fn baseline_op_id(block_id: BlockId, char_pos: usize) -> OpId {
    let mut data = block_id.as_bytes().to_vec();
    data.extend_from_slice(&(char_pos as u64).to_le_bytes());
    let site_uuid = Uuid::new_v5(&BASELINE_NS, &data);
    OpId::new(SiteId(site_uuid), Lamport(0))
}

/// Deterministic [`OpId`] for the `char_offset`-th character of a
/// multi-character authoring op whose envelope id is `envelope`.
///
/// `char_offset == 0` returns `envelope` unchanged (so single-character
/// inserts and the *first* character of multi-char inserts share the
/// envelope id — keeping the wire op self-identifying).
#[must_use]
pub fn subop_id(envelope: OpId, char_offset: usize) -> OpId {
    if char_offset == 0 {
        return envelope;
    }
    let mut data = envelope.site.0.as_bytes().to_vec();
    data.extend_from_slice(&envelope.lamport.0.to_le_bytes());
    data.extend_from_slice(&(char_offset as u64).to_le_bytes());
    let site_uuid = Uuid::new_v5(&SUBOP_NS, &data);
    OpId::new(SiteId(site_uuid), envelope.lamport)
}

/// Return the **character** index in `content` corresponding to the
/// given byte position. Saturates to the visible char count when
/// `byte_pos` reaches the end of the string.
#[must_use]
pub fn byte_to_char_pos(content: &str, byte_pos: usize) -> usize {
    if byte_pos == 0 {
        return 0;
    }
    let mut count = 0;
    for (b, _) in content.char_indices() {
        if b >= byte_pos {
            return count;
        }
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn baseline_is_deterministic_across_calls() {
        let block = Uuid::new_v4();
        assert_eq!(baseline_op_id(block, 7), baseline_op_id(block, 7));
        assert_ne!(baseline_op_id(block, 7), baseline_op_id(block, 8));
    }

    #[test]
    fn baseline_lamport_is_zero() {
        let block = Uuid::new_v4();
        assert_eq!(baseline_op_id(block, 0).lamport, Lamport(0));
    }

    #[test]
    fn subop_offset_zero_is_envelope() {
        let env = OpId::new(SiteId::new(), Lamport(5));
        assert_eq!(subop_id(env, 0), env);
    }

    #[test]
    fn subop_offsets_differ_within_one_envelope() {
        let env = OpId::new(SiteId::new(), Lamport(5));
        let a = subop_id(env, 1);
        let b = subop_id(env, 2);
        assert_ne!(a, b);
        assert_eq!(a.lamport, env.lamport);
        assert_eq!(b.lamport, env.lamport);
    }

    #[test]
    fn byte_to_char_handles_multibyte() {
        let s = "aé😀b"; // 1 + 2 + 4 + 1 = 8 bytes; 4 chars.
        assert_eq!(byte_to_char_pos(s, 0), 0);
        assert_eq!(byte_to_char_pos(s, 1), 1); // after 'a'
        assert_eq!(byte_to_char_pos(s, 3), 2); // after 'é'
        assert_eq!(byte_to_char_pos(s, 7), 3); // after '😀'
        assert_eq!(byte_to_char_pos(s, 8), 4); // end
        assert_eq!(byte_to_char_pos(s, 999), 4); // saturates
    }
}
