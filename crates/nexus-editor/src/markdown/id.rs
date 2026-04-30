//! Deterministic block-id generation for markdown parsing.
//!
//! The PRD's sample at [docs/PRDs/08-editor-engine.md:542] hashes only
//! `(content, type)` which collides on duplicate paragraphs. We
//! additionally mix in the document path and the block's pre-order
//! position so every slot gets a stable, unique UUID.
//!
//! The lazy stamping path described in ADR 0017 lives in this module
//! too: [`parse_stable_id_marker`] recognizes a `<!-- ^<uuid> -->`
//! comment, and [`format_stable_id_marker`] emits one. The serializer
//! attaches the marker to a block when [`crate::Block::stable_id`] is
//! `Some`, and the parser strips it back out into `stable_id` so the
//! id survives edits upstream of the block.

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

// ── Stable-id marker (ADR 0017) ──────────────────────────────────────────────

/// Match a single `<!-- ^<uuid> -->` HTML-comment marker, returning
/// the parsed [`BlockId`].
///
/// The match is whole-string: leading and trailing ASCII whitespace
/// is tolerated so the helper accepts both an inline trailing form
/// (`Hello world <!-- ^…uuid… -->`) — passed in after the caller has
/// sliced out the trailing comment — and a block-level form
/// (`<!-- ^…uuid… -->\n`) emitted as its own line. The uuid must be a
/// canonical hyphenated v4 string; anything else returns `None`.
#[must_use]
pub fn parse_stable_id_marker(s: &str) -> Option<BlockId> {
    let trimmed = s.trim();
    let inner = trimmed.strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    let uuid_part = inner.strip_prefix('^')?.trim();
    BlockId::parse_str(uuid_part).ok()
}

/// Find a trailing `<!-- ^<uuid> -->` marker at the end of `s` and
/// return `(prefix_without_marker, parsed_id)` when present.
///
/// Used by the inline-content path: after [`super::inline::collect_inline`]
/// assembles plain text that may end with a stamp marker, this helper
/// strips the marker (and the single space that the serializer always
/// emits before it) before the cleaned content is stored on the block.
#[must_use]
pub fn strip_trailing_stable_id_marker(s: &str) -> Option<(String, BlockId)> {
    let pos = s.rfind("<!--")?;
    let id = parse_stable_id_marker(&s[pos..])?;
    let mut head = s[..pos].to_string();
    // The serializer always writes a single space between the
    // user-visible content and the marker; strip it back so the
    // round-trip is lossless.
    if head.ends_with(' ') {
        head.pop();
    }
    Some((head, id))
}

/// Format a stamp marker for `id`. Inverse of [`parse_stable_id_marker`].
#[must_use]
pub fn format_stable_id_marker(id: &BlockId) -> String {
    format!("<!-- ^{id} -->")
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

    #[test]
    fn parse_stable_id_marker_round_trips() {
        let id = BlockId::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let s = format_stable_id_marker(&id);
        assert_eq!(s, "<!-- ^550e8400-e29b-41d4-a716-446655440000 -->");
        assert_eq!(parse_stable_id_marker(&s), Some(id));
    }

    #[test]
    fn parse_stable_id_marker_tolerates_surrounding_whitespace() {
        let id = BlockId::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let s = "  <!-- ^550e8400-e29b-41d4-a716-446655440000 -->  \n";
        assert_eq!(parse_stable_id_marker(s), Some(id));
    }

    #[test]
    fn parse_stable_id_marker_rejects_bad_uuid() {
        assert!(parse_stable_id_marker("<!-- ^not-a-uuid -->").is_none());
        assert!(parse_stable_id_marker("<!-- ^ -->").is_none());
        assert!(parse_stable_id_marker("<!--550e8400-e29b-41d4-a716-446655440000-->").is_none());
    }

    #[test]
    fn parse_stable_id_marker_rejects_unrelated_html_comments() {
        assert!(parse_stable_id_marker("<!-- TODO -->").is_none());
        assert!(parse_stable_id_marker("Hello").is_none());
        assert!(parse_stable_id_marker("").is_none());
    }

    #[test]
    fn strip_trailing_stable_id_marker_extracts_and_drops_space() {
        let id = BlockId::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let raw = format!("Hello world {}", format_stable_id_marker(&id));
        let (head, parsed) = strip_trailing_stable_id_marker(&raw).unwrap();
        assert_eq!(head, "Hello world");
        assert_eq!(parsed, id);
    }

    #[test]
    fn strip_trailing_stable_id_marker_no_marker_returns_none() {
        assert!(strip_trailing_stable_id_marker("just plain text").is_none());
    }

    #[test]
    fn strip_trailing_stable_id_marker_ignores_inner_html_comments() {
        // A comment in the middle of content should NOT be stripped.
        let s = "Hello <!-- not-a-stamp --> world";
        assert!(strip_trailing_stable_id_marker(s).is_none());
    }
}
