//! Conflict descriptors surfaced by [`crate::CrdtDoc`] when two
//! concurrent ops cannot be merged silently.
//!
//! Phase 1 (current) flags concurrent in-block edits as conflicts so
//! the calling layer can decide what to do. Phase 2 will resolve text
//! conflicts internally via [`crate::text::RgaText`] and only structural
//! conflicts (delete + edit on the same block) will reach the caller.

use nexus_editor::BlockId;
use serde::{Deserialize, Serialize};

use crate::id::OpId;

/// A merge situation that the Phase 1 doc cannot resolve on its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Conflict {
    /// Two sites concurrently edited the *content* of the same block.
    /// The local op was already applied; the remote op is buffered for
    /// the resolver. Phase 2 replaces this with a silent RGA merge for
    /// pure text overlap.
    ConcurrentBlockEdit {
        /// Block whose content diverged.
        block_id: BlockId,
        /// Op id already applied locally.
        local: OpId,
        /// Op id of the conflicting remote op.
        remote: OpId,
    },
    /// One site deleted a block while another site edited it. There is
    /// no automatic resolution — the user must choose to keep the
    /// edit (cancel the delete) or accept the delete (drop the edit).
    /// Always surfaces, even after Phase 2.
    StructuralDeleteEdit {
        /// Block the conflict is about.
        block_id: BlockId,
        /// The delete op id.
        delete: OpId,
        /// The edit op id.
        edit: OpId,
    },
}

impl Conflict {
    /// The block this conflict is about.
    #[must_use]
    pub fn block_id(&self) -> BlockId {
        match self {
            Self::ConcurrentBlockEdit { block_id, .. }
            | Self::StructuralDeleteEdit { block_id, .. } => *block_id,
        }
    }
}
