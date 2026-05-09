//! Error types for the CRDT layer.

use nexus_editor::EditorError;
use thiserror::Error;
use uuid::Uuid;

use crate::id::OpId;

/// Crate-local result alias.
pub type Result<T> = std::result::Result<T, CrdtError>;

/// Errors raised by [`crate::CrdtDoc`] and the op log.
#[derive(Debug, Error)]
pub enum CrdtError {
    /// A remote op arrived before the ops it causally depends on.
    /// The doc has buffered it and will retry once its dependencies
    /// are satisfied.
    #[error("op {0:?} is causally pending")]
    CausallyPending(OpId),

    /// A structural conflict (concurrent delete + edit on the same
    /// block) needs caller-side resolution. The CRDT cannot decide
    /// silently — see ADR 0026 §3.
    #[error("structural conflict on block {block_id}: {reason}")]
    StructuralConflict {
        /// The block in conflict.
        block_id: Uuid,
        /// Human-readable description for the resolution UI.
        reason: String,
    },

    /// The wrapped editor operation failed to apply (block missing,
    /// position out of range, etc.). This usually means the remote
    /// peer has a divergent view that this Phase 1 doc cannot merge —
    /// caller should surface to the user.
    #[error("editor operation failed: {0}")]
    Editor(#[from] EditorError),

    /// A wire-format payload could not be encoded or decoded. Phase 3+
    /// transports return this when an event-bus payload doesn't match
    /// the expected schema (see [`crate::wire`]).
    #[error("wire payload: {0}")]
    Wire(String),
}
