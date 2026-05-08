//! Operation-based CRDT layer for collaborative editing (BL-074, PRD-08 §8).
//!
//! This crate wraps [`nexus_editor::Operation`] in a CRDT envelope so two
//! sessions on the same forge can exchange edits and converge without
//! user intervention.
//!
//! # Phase 1 scope (this revision)
//!
//! - Core types: [`SiteId`], [`Lamport`], [`OpId`], [`VersionVector`].
//! - [`OpLog`]: append-only, idempotent by `OpId`, with version-vector
//!   summary for gossip.
//! - [`CrdtDoc`]: tracks a [`BlockTree`] plus its op log, applies local
//!   and remote ops, and surfaces concurrent same-block edits as
//!   [`Conflict`] values for the caller (UI / sync layer) to resolve.
//! - [`text::RgaText`]: sequence CRDT for in-block character-level merge,
//!   tested standalone. Phase 2 wires it into `CrdtDoc`'s text-conflict
//!   path so concurrent text edits within a block converge automatically
//!   instead of surfacing as a conflict.
//!
//! # Phase 2 (deferred — see ADR 0026)
//!
//! - Wire [`text::RgaText`] into block-content merge so concurrent
//!   `InsertText` / `DeleteText` on the same block converge silently.
//! - Reparenting / move-loop detection.
//! - Event-bus sync loop (`com.nexus.editor.ops.<path>`) and Tauri
//!   transport for live cursor exchange.
//! - BL-007 git-on-disk persistence of op log + version vector.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod conflict;
mod doc;
mod error;
mod id;
mod log;
mod op;
pub mod text;

pub use conflict::Conflict;
pub use doc::CrdtDoc;
pub use error::{CrdtError, Result};
pub use id::{Lamport, OpId, SiteId, VersionVector};
pub use log::OpLog;
pub use op::{affected_blocks, primary_block_id, CrdtOp};
