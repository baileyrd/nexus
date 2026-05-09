//! Conflict descriptors surfaced by [`crate::CrdtDoc`] when two
//! concurrent ops cannot be merged silently.
//!
//! Phase 1 (current) flags concurrent in-block edits as conflicts so
//! the calling layer can decide what to do. Phase 2 will resolve text
//! conflicts internally via [`crate::text::RgaText`] and only structural
//! conflicts (delete + edit on the same block) will reach the caller.
//!
//! # Resolver-friendly form
//!
//! [`Conflict`] is the lightweight discriminator the doc returns from
//! [`crate::CrdtDoc::apply_remote`] — it carries op ids only. The
//! BL-074 resolver modal needs richer context (the actual block
//! content on each side) to render side-by-side, so the BL-007
//! pull-landing path enriches each conflict with a [`ConflictDetail`]
//! before publishing it on the conflict topic. That gives the shell
//! everything it needs to draw "Keep local" / "Use remote" without an
//! extra round-trip. See `crate::wire::ConflictEnvelope`.

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

/// Which side of a merge an op originated from, from the live
/// session's point of view.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictOrigin {
    /// The op is in the live session's op log — authored locally.
    Local,
    /// The op arrived via the remote state file — authored elsewhere
    /// and absorbed by `git pull` + the BL-007 merge driver.
    Remote,
}

/// [`Conflict`] enriched with the content snapshots a resolver UI
/// needs to render side-by-side. Built by the BL-007 pull-landing
/// path before publishing on `com.nexus.editor.crdt.conflict.<relpath>`.
///
/// The wire form flattens `Conflict`'s fields up so existing
/// subscribers (the BL-074 conflict toast) keep parsing without
/// changes — the new fields are all optional and additive.
///
/// `local_content` / `remote_content` are populated for the
/// content-bearing variants ([`Operation::UpdateBlockContent`] in
/// particular). For [`Conflict::StructuralDeleteEdit`] the surviving
/// content lands in `local_content` (whichever side has the edit) and
/// `delete_origin` records who issued the delete; `remote_content` is
/// `None` because the delete carries no replacement text.
///
/// [`Operation::UpdateBlockContent`]: nexus_editor::Operation::UpdateBlockContent
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConflictDetail {
    /// Underlying conflict descriptor. Flattened so the JSON keeps the
    /// pre-BL-074 shape (`kind`, `block_id`, `local`, `remote`, …).
    #[serde(flatten)]
    pub conflict: Conflict,
    /// Block content on the local side at conflict time. For
    /// [`Conflict::ConcurrentBlockEdit`] this is the live tree's
    /// content; for [`Conflict::StructuralDeleteEdit`] it's the
    /// surviving edit's content (possibly recovered from the buffered
    /// edit op when the local side was the deleter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_content: Option<String>,
    /// Block content the remote op would have produced. `None` when
    /// the remote op doesn't carry a content payload (e.g.,
    /// [`Operation::UpdateAnnotations`] — annotations diverged but
    /// content didn't) or for delete-edit conflicts (no replacement).
    ///
    /// [`Operation::UpdateAnnotations`]: nexus_editor::Operation::UpdateAnnotations
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_content: Option<String>,
    /// For [`Conflict::StructuralDeleteEdit`] only: which side issued
    /// the delete. `None` for [`Conflict::ConcurrentBlockEdit`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_origin: Option<ConflictOrigin>,
}

impl ConflictDetail {
    /// Construct a detail with no content snapshots — equivalent to
    /// the pre-BL-074 toast payload. Useful for tests and for the
    /// fallback path in [`crate::wire::ConflictEnvelope::new`].
    #[must_use]
    pub fn bare(conflict: Conflict) -> Self {
        Self {
            conflict,
            local_content: None,
            remote_content: None,
            delete_origin: None,
        }
    }

    /// The block this conflict is about (delegates to the wrapped
    /// [`Conflict`]).
    #[must_use]
    pub fn block_id(&self) -> BlockId {
        self.conflict.block_id()
    }
}

impl From<Conflict> for ConflictDetail {
    fn from(conflict: Conflict) -> Self {
        Self::bare(conflict)
    }
}
