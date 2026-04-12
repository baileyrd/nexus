//! Nexus storage engine: forge layout, atomic writes, `SQLite` index,
//! markdown parsing, file watching, and Tantivy full-text search.
//!
//! See `docs/superpowers/specs/2026-04-12-nexus-prd-03-storage-design.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod error;
mod forge;
mod atomic;

pub use atomic::atomic_write;
pub use error::StorageError;
pub use forge::{Forge, ForgeLock};
