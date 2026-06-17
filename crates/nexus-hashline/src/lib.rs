//! # nexus-hashline
//!
//! The **hashline** patch format — content-hash-anchored edits that bind a patch
//! to the exact file state the author last read, so whitespace drift and
//! "string not found" retry loops stop costing correctness. This is the
//! omp-style edit format described in [RFC 0005] (Phase 5.1).
//!
//! [RFC 0005]: ../../../docs/0.1.2/rfcs/0005-omp-agentic-loop-phase5.md
//!
//! ## Shape of a patch
//!
//! A patch is one or more file sections. Each section header is `[PATH#TAG]`,
//! where `TAG` is a 4-uppercase-hex content hash of the *normalized* file text
//! ([`tag`]). The body is a sequence of line/insert operations:
//!
//! ```text
//! [src/main.rs#1A2B]
//! SWAP 10.=12:
//! +    let x = compute();
//! +    use_it(x);
//! DEL 20.=21
//! INS.PRE 1:
//! +//! prepended doc line
//! ```
//!
//! Body rows are prefixed with `+`; a bare `+` is a blank line. The leading `+`
//! is a row marker, so decoding a row is simply "strip exactly one `+`" — a
//! content line that itself starts with `+` is written `++…` and round-trips.
//!
//! ## Applying a patch
//!
//! [`apply_section`] is the high-level entry point. It compares the section's
//! `TAG` against the live file:
//!
//! * **match** → apply the operations directly ([`EditOutcome::Applied`]);
//! * **mismatch** → reconstruct the base the author saw from a [`SnapshotStore`]
//!   and run a **3-way merge** (base / patched-base / current). A clean merge is
//!   [`EditOutcome::Merged`]; an unresolvable one is [`EditOutcome::Conflict`]
//!   with diff3 conflict markers. With no recorded base, [`HashlineError::StaleTag`].
//!
//! ## Scope (Phase 5.1)
//!
//! Line and whole-file insert operations are implemented. The block variants
//! (`SWAP.BLK`, `DEL.BLK`, `INS.BLK.POST`) parse — so the grammar stays
//! forward-compatible — but applying them returns
//! [`HashlineError::BlockOpsUnsupported`] until tree-sitter lands (Phase 5.2).
//!
//! This crate is a **leaf**: it depends only on `sha2` (TAG hashing), `diffy`
//! (3-way merge), and `thiserror`. It has no knowledge of the kernel, the forge,
//! or IPC — `nexus-storage` wires it into a `com.nexus.storage::edit` handler.

#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![forbid(unsafe_code)]

mod apply;
mod error;
mod parse;
mod snapshot;
mod tag;

pub use apply::{apply_ops, apply_section, EditOutcome};
pub use error::HashlineError;
pub use parse::{parse, FileSection, Op, Patch};
pub use snapshot::{
    Snapshot, SnapshotStore, MAX_SNAPSHOT_BYTES, MAX_SNAPSHOT_PATHS, MAX_VERSIONS_PER_PATH,
};
pub use tag::{normalize, tag, tag_matches, TAG_HEX_LEN};
