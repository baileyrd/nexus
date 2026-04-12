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
pub(crate) mod schema;
mod parser;
mod index;
mod search;

pub use atomic::atomic_write;
pub use error::StorageError;
pub use forge::{Forge, ForgeLock};
pub use parser::{content_hash, parse_markdown, ParsedBlock, ParsedFile, ParsedLink, ParsedTag, Property};
pub use index::{BlockRecord, FileFilter, FileMetadata, FileRecord, LinkRecord, RebuildStats, TagResult};
pub use index::{insert_file, query_files, query_blocks, query_links, query_backlinks, query_tags, delete_file, soft_delete_file, file_by_path};
pub use search::{SearchIndex, SearchResult};
