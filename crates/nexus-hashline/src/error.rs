//! Error type for hashline parsing and application.

use thiserror::Error;

/// Everything that can go wrong parsing or applying a hashline patch.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HashlineError {
    /// A `[PATH#TAG]` header was malformed (missing `#`, empty path, or a TAG
    /// that is not exactly four hex digits).
    #[error("malformed section header on line {line}: expected `[PATH#TAG]` with a 4-hex TAG")]
    BadSectionHeader {
        /// 1-based line number within the patch text.
        line: usize,
    },

    /// An operation header could not be parsed.
    #[error("malformed operation on line {line}: {detail}")]
    BadOp {
        /// 1-based line number within the patch text.
        line: usize,
        /// Human-readable reason.
        detail: String,
    },

    /// A line range `A.=B` was empty or inverted (`B < A`, or a zero index).
    #[error("line range {start}.={end} is empty, inverted, or zero-indexed")]
    BadRange {
        /// Range start (as written).
        start: usize,
        /// Range end (as written).
        end: usize,
    },

    /// An operation referenced a line outside the target file.
    #[error("operation references line {line}, but the file has {len} line(s)")]
    LineOutOfBounds {
        /// The offending 1-based line number.
        line: usize,
        /// Number of lines in the target file.
        len: usize,
    },

    /// Two operations in one section touched the same line.
    #[error("operations overlap at line {line}")]
    OverlappingOps {
        /// The 1-based line both operations claimed.
        line: usize,
    },

    /// A block operation (`SWAP.BLK` / `DEL.BLK` / `INS.BLK.POST`) was applied;
    /// these need tree-sitter, which is not wired yet (Phase 5.2).
    #[error("block operations require tree-sitter, which is not available yet (Phase 5.2)")]
    BlockOpsUnsupported,

    /// The patch TAG did not match the current file and no snapshot of the
    /// author's base was available to drive a 3-way merge.
    #[error(
        "patch TAG {patch_tag} does not match the current file (TAG {current_tag}) \
         and no snapshot is available for a 3-way merge"
    )]
    StaleTag {
        /// TAG the patch claimed.
        patch_tag: String,
        /// TAG the live file actually hashes to.
        current_tag: String,
    },
}
