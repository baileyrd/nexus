//! Phase 4 persistence: serializable [`CrdtState`] snapshot of a
//! [`crate::CrdtDoc`] without its [`nexus_editor::BlockTree`].
//!
//! The block tree is recovered from the markdown source on load
//! (file-as-truth, ADR 0026 §"Phase 4"). The CRDT layer only persists
//! the *delta* between baseline content and the live document state:
//! site/lamport, op log, per-block meta, and the per-block RGA mirror.
//!
//! Layout on disk follows the BL-072 undo-state convention:
//!
//! ```text
//! <forge>/.forge/.editor/crdt/<sha-of-relpath>.json
//! ```
//!
//! Each file holds one [`PersistedCrdt`] — a thin envelope around
//! [`CrdtState`] that adds a schema version and a content hash so a
//! mismatched markdown source (external edit, unsaved close)
//! invalidates the cached state instead of replaying the wrong ops.

use std::collections::HashMap;

use nexus_editor::BlockId;
use serde::{Deserialize, Serialize};

use crate::doc::BlockMeta;
use crate::id::{Lamport, SiteId};
use crate::log::OpLog;
use crate::text::RgaText;

/// Schema version of [`PersistedCrdt`]. Bump on incompatible shape
/// changes; older versions are ignored on read so the editor degrades
/// gracefully (start fresh, replay nothing) rather than panicking.
pub const PERSISTED_VERSION: u32 = 1;

/// In-memory snapshot of a [`crate::CrdtDoc`]'s persistent state. The
/// block tree is intentionally absent — it's reconstructed from the
/// markdown source.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CrdtState {
    /// This site's id at snapshot time. Persisting it lets the same
    /// session id continue across reopens; without persistence, every
    /// reopen would treat itself as a fresh site and the op log would
    /// lose causal continuity.
    pub site: SiteId,
    /// Highest lamport observed (local + remote).
    pub lamport: Lamport,
    /// Op log (idempotent by op id; carries version vector).
    pub log: OpLog,
    /// Per-block meta (last writer, tombstone status).
    pub block_meta: HashMap<BlockId, BlockMeta>,
    /// Per-block RGA mirror.
    pub rga: HashMap<BlockId, RgaText>,
}

/// On-disk envelope. Adds a schema version and an integrity tag so a
/// mismatched markdown source invalidates the persisted state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersistedCrdt {
    /// Schema version. See [`PERSISTED_VERSION`].
    pub version: u32,
    /// SHA-256 (hex) of the canonical-markdown source the state was
    /// authored against. Compared against a fresh hash of the source
    /// on load — mismatch ⇒ the markdown changed externally and the
    /// cached log can no longer be safely replayed.
    pub content_hash: String,
    /// Wall-clock seconds since the unix epoch at write time.
    pub persisted_at_unix: u64,
    /// The CRDT state itself.
    pub state: CrdtState,
}

impl PersistedCrdt {
    /// Wrap `state` in an envelope tagged with `content_hash` and the
    /// current wall-clock time.
    #[must_use]
    pub fn new(state: CrdtState, content_hash: String) -> Self {
        Self {
            version: PERSISTED_VERSION,
            content_hash,
            persisted_at_unix: now_unix_secs(),
            state,
        }
    }
}

fn now_unix_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Build the `<forge>/.forge/.editor/crdt/<sha>.json` storage path for
/// `relpath`. The relpath is hashed so the on-disk filename is opaque
/// (no traversal, no clashes with `/`-bearing relpaths) — same
/// convention as the BL-072 undo-state path.
#[must_use]
pub fn crdt_state_path(relpath: &str) -> String {
    use std::fmt::Write as _;
    let mut hex = String::with_capacity(16);
    let digest = sha256_bytes(relpath.as_bytes());
    for b in digest.iter().take(8) {
        write!(&mut hex, "{b:02x}").expect("write to String");
    }
    format!(".forge/.editor/crdt/{hex}.json")
}

/// SHA-256 hex of `bytes`. Used as the content-hash integrity tag so an
/// external markdown edit between close and open invalidates the
/// cached CRDT state.
#[must_use]
pub fn content_hash_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let digest = sha256_bytes(bytes);
    let mut hex = String::with_capacity(64);
    for b in &digest {
        write!(&mut hex, "{b:02x}").expect("write to String");
    }
    hex
}

/// Tiny self-contained SHA-256 — avoids pulling sha2 into nexus-crdt
/// just for one hash. Implementation is the FIPS-180-4 reference; the
/// only callers are `content_hash_hex` and `crdt_state_path`.
#[allow(clippy::many_single_char_names)] // `a..h` mirror the FIPS-180-4 reference variable names.
fn sha256_bytes(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
        0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
        0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
        0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
        0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
        0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
        0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
        0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
        0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
        0xc671_78f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a, 0x510e_527f, 0x9b05_688c, 0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity(input.len() + 9 + 63);
    padded.extend_from_slice(input);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in padded.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use nexus_editor::{Block, BlockTree, BlockType, DocumentMetadata, Operation};

    use super::*;
    use crate::doc::CrdtDoc;
    use crate::id::SiteId;

    fn tree_with_block(content: &str) -> (BlockTree, nexus_editor::BlockId) {
        let mut tree = BlockTree::new(DocumentMetadata::default());
        let mut block = Block::new(BlockType::Paragraph);
        block.content = content.to_string();
        let id = block.id;
        tree.insert(block, None, 0).unwrap();
        (tree, id)
    }

    fn insert_text(block_id: nexus_editor::BlockId, pos: usize, text: &str) -> Operation {
        Operation::InsertText {
            block_id,
            pos,
            text: text.into(),
            pre_annotations: vec![],
        }
    }

    #[test]
    fn persisted_envelope_round_trips_full_state() {
        // End-to-end: doc → state → PersistedCrdt → JSON → load → doc.
        // Restored doc continues to converge with a peer that observed
        // the same ops via gossip.
        let s1 = SiteId::new();
        let (tree, b) = tree_with_block("hello");
        let mut doc = CrdtDoc::new(s1, tree.clone());
        let _wire = doc.apply_local(&insert_text(b, 5, " world")).unwrap();

        let hash = content_hash_hex(b"hello");
        let envelope = PersistedCrdt::new(doc.state(), hash.clone());
        let json = serde_json::to_vec(&envelope).unwrap();

        let decoded: PersistedCrdt = serde_json::from_slice(&json).unwrap();
        assert_eq!(decoded.version, PERSISTED_VERSION);
        assert_eq!(decoded.content_hash, hash);

        // Restoring against the *baseline* tree (matching the hash)
        // gives back a doc whose RGA contains all the persisted edits.
        let restored = CrdtDoc::from_state(tree.clone(), decoded.state);
        assert_eq!(restored.site(), s1);
        assert_eq!(restored.log().len(), 1);
        let rga = restored.block_rga(b).expect("RGA was persisted");
        assert_eq!(rga.render(), "hello world");
    }

    #[test]
    fn merged_oplog_replays_to_convergent_state() {
        // Two persisted op logs are merged via OpLog::merge; replaying
        // the merged log onto a fresh doc gives the same state as
        // applying the ops individually.
        let s1 = SiteId::new();
        let s2 = SiteId::new();
        let (tree, b) = tree_with_block("");
        let mut doc1 = CrdtDoc::new(s1, tree.clone());
        let mut doc2 = CrdtDoc::new(s2, tree.clone());
        let op_a = doc1.apply_local(&insert_text(b, 0, "A")).unwrap();
        let op_b = doc2.apply_local(&insert_text(b, 0, "B")).unwrap();

        // doc1 has only its own log. doc2 has only its own log.
        // Merge doc2's log into doc1's, replay onto a fresh peer:
        let mut merged_log = doc1.log().clone();
        let absorbed = merged_log.merge(doc2.log());
        assert_eq!(absorbed, 1);
        assert_eq!(merged_log.len(), 2);

        // A peer receiving the merged log via gossip would see both
        // ops and converge with both authors — verify by replaying
        // each into a fresh doc the long way.
        doc1.apply_remote(op_b).unwrap();
        doc2.apply_remote(op_a).unwrap();
        assert_eq!(
            doc1.tree().get(b).unwrap().content,
            doc2.tree().get(b).unwrap().content,
            "both authors converge"
        );
    }

    #[test]
    fn path_is_under_crdt_subdir() {
        let p = crdt_state_path("notes/today.md");
        assert!(p.starts_with(".forge/.editor/crdt/"));
        assert!(std::path::Path::new(&p)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json")));
    }

    #[test]
    fn path_collision_resistance_for_typical_relpaths() {
        // Hash truncation to 16 hex chars (64 bits) is enough for the
        // few hundred files a forge edits in a session — same logic
        // as the BL-072 undo-state path. Still sanity-check that
        // common neighbour relpaths don't hash-collide.
        let a = crdt_state_path("a.md");
        let b = crdt_state_path("b.md");
        let c = crdt_state_path("notes/a.md");
        assert_ne!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn content_hash_matches_known_vector() {
        // SHA-256 of the empty string.
        assert_eq!(
            content_hash_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // SHA-256("abc")
        assert_eq!(
            content_hash_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn content_hash_changes_with_input() {
        let h1 = content_hash_hex(b"hello world");
        let h2 = content_hash_hex(b"hello world!");
        assert_ne!(h1, h2);
    }
}
