//! Nexus editor engine: block tree, annotations, transactions.
//!
//! Implements the in-memory domain model described in
//! `docs/PRDs/08-editor-engine.md` §§1 (Block Tree), 2 (Annotations),
//! 3 (Markdown ↔ Block Tree Roundtrip) and 5 (Transactions &
//! Undo/Redo). `CodeMirror` 6 integration (§4) and slash-command
//! dispatch (§6) live elsewhere.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod annotation;
mod block;
pub mod core_plugin;
mod error;
pub mod markdown;
mod transaction;
mod tree;
mod undo_tree;

pub use annotation::{adjust_annotations, merge as merge_annotations, Annotation, AnnotationType};
pub use block::{
    now_ms, Block, BlockId, BlockProperties, BlockType, DatabaseViewConfig, DatabaseViewType,
    DocumentMetadata, EmbedType, FileType, PropertyValue,
};
pub use error::{EditorError, Result};
pub use transaction::{
    BlockOp, Operation, Transaction, TransactionMetadata, TransactionSource, UserAction,
};
pub use tree::BlockTree;
pub use undo_tree::UndoTree;

pub use markdown::{MarkdownParser, MarkdownSerializer, ParseOptions};

pub use core_plugin::{EditorCorePlugin, EditorSnapshot, PLUGIN_ID as EDITOR_PLUGIN_ID};
