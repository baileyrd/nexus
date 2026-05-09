//! Operation-based CRDT layer for collaborative editing (BL-074, PRD-08 §8).
//!
//! This crate wraps [`nexus_editor::Operation`] in a CRDT envelope so two
//! sessions on the same forge can exchange edits and converge without
//! user intervention.
//!
//! # Scope (Phase 1 + Phase 2)
//!
//! - Core types: [`SiteId`], [`Lamport`], [`OpId`], [`VersionVector`].
//! - [`OpLog`]: append-only, idempotent by `OpId`, with version-vector
//!   summary for gossip.
//! - [`CrdtDoc`]: tracks a [`BlockTree`] plus its op log and a
//!   per-block [`text::RgaText`] mirror. Local edits author wire ops
//!   ([`CrdtOp`]) carrying both the editor [`nexus_editor::Operation`]
//!   and a position-free RGA translation; remote edits apply through
//!   the RGA when concurrent so text overlap converges silently.
//! - [`text::RgaText`]: sequence CRDT for in-block character-level
//!   merge.
//! - [`merge`]: deterministic synthetic-id helpers that let two peers
//!   independently materialise the same baseline RGA.
//!
//! After Phase 2 the only conflict surface that reaches the caller is
//! [`Conflict::StructuralDeleteEdit`] (edit racing a block delete) and
//! [`Conflict::ConcurrentBlockEdit`] for concurrent whole-block
//! replacements (`UpdateBlockContent` / `UpdateAnnotations`) — those
//! aren't the RGA's problem.
//!
//! # Deferred (see ADR 0026)
//!
//! - Phase 3 — event-bus sync loop (`com.nexus.editor.ops.<path>`) and
//!   Tauri transport for live cursor exchange.
//! - Phase 4 — BL-007 git-on-disk persistence of op log + version
//!   vector (load/save lives in the editor; merge driver in storage).
//! - Reparenting / move-loop detection.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod conflict;
mod doc;
mod error;
mod id;
mod log;
pub mod merge;
mod op;
pub mod state;
pub mod sync;
pub mod text;
pub mod wire;

pub use conflict::Conflict;
pub use doc::{BlockMeta, CrdtDoc, RemoteOutcome};
pub use error::{CrdtError, Result};
pub use id::{Lamport, OpId, SiteId, VersionVector};
pub use log::OpLog;
pub use op::{affected_blocks, primary_block_id, CrdtOp};
pub use state::{
    content_hash_hex, crdt_state_path, CrdtState, PersistedCrdt, PERSISTED_VERSION,
};
pub use sync::{DocHandle, InboundOutcome, SyncLoop};
pub use wire::{ops_topic, relpath_of_topic, OpEnvelope, OPS_TOPIC_PREFIX};
