//! BL-050 — side-margin comments subsystem.
//!
//! Persistent comment threads anchored to stable block ids
//! (see ADR 0017). Each markdown file gets a JSON sidecar at
//! `<forge>/.forge/comments/<relpath>.json` keyed on the file's
//! forge-relative path; the sidecar mirrors the directory structure
//! of the source file so two files with the same basename in
//! different directories don't collide.
//!
//! The store is **file-as-truth** in spirit: the JSON sidecar is the
//! source of record for thread state; threads are anchored to
//! `block_id` (Uuid) values that the editor stamps lazily into the
//! markdown source via `<!-- ^<uuid> -->` markers.
//!
//! This crate is intentionally storage-only — no UI. Mutations do
//! publish `com.nexus.comments.*` events on the kernel bus (C60 /
//! #413) so collab peers, popout windows, and any other IPC-only
//! writer can react to thread changes without polling.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]

pub mod core_plugin;
mod store;
mod types;

pub use store::{CommentStore, CommentStoreError};
pub use types::{Comment, CommentFile, CommentId, Thread, ThreadId};
