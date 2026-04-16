//! Deterministic block-id generation for markdown parsing.
//!
//! The PRD's sample at [docs/PRDs/08-editor-engine.md:542] hashes only
//! `(content, type)` which collides on duplicate paragraphs. We
//! additionally mix in the document path and the block's pre-order
//! position so every slot gets a stable, unique UUID.

use sha2::{Digest, Sha256};

use crate::block::{BlockId, BlockType};

/// Compute a deterministic [`BlockId`] for a block.
///
/// Inputs that vary the id:
/// - `file_path` — blocks from different files never collide.
/// - `visit_order` — blocks at different slots in the same file never
///   collide.
/// - `ty` — structural changes produce different ids even when
///   `visit_order` is unchanged.
///
/// The result is a valid RFC 4122 v4 UUID.
#[must_use]
pub fn deterministic_block_id(file_path: &str, visit_order: usize, ty: &BlockType) -> BlockId {
    let mut h = Sha256::new();
    h.update(b"nexus-editor:v1:");
    h.update(file_path.as_bytes());
    h.update(b"|");
    h.update(visit_order.to_le_bytes());
    h.update(b"|");
    h.update(format!("{ty:?}").as_bytes());
    let digest = h.finalize();

    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[0..16]);
    // Force RFC 4122 v4 version bits so the output is a valid Uuid.
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    BlockId::from_bytes(bytes)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_inputs_produce_same_id() {
        let a = deterministic_block_id("file.md", 0, &BlockType::Paragraph);
        let b = deterministic_block_id("file.md", 0, &BlockType::Paragraph);
        assert_eq!(a, b);
    }

    #[test]
    fn different_slots_produce_different_ids() {
        let a = deterministic_block_id("file.md", 0, &BlockType::Paragraph);
        let b = deterministic_block_id("file.md", 1, &BlockType::Paragraph);
        assert_ne!(a, b);
    }

    #[test]
    fn different_files_produce_different_ids() {
        let a = deterministic_block_id("a.md", 0, &BlockType::Paragraph);
        let b = deterministic_block_id("b.md", 0, &BlockType::Paragraph);
        assert_ne!(a, b);
    }

    #[test]
    fn different_types_produce_different_ids() {
        let a = deterministic_block_id("f.md", 0, &BlockType::Paragraph);
        let b = deterministic_block_id("f.md", 0, &BlockType::Heading { level: 1 });
        assert_ne!(a, b);
    }
}
