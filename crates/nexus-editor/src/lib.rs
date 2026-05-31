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
pub mod database_view;
mod error;
// #202 / R19 — `excerpt_map` is the Step-1 primitive layer of an
// in-progress excerpt-mapping feature; Step 2 (the `apply_transaction`
// wire-up) hasn't landed yet so every item in the module is currently
// uncalled. The `#[allow(dead_code)]` lives here, on the module
// declaration, rather than as an inner attribute inside the file —
// the original `#![allow(dead_code)]` was flagged by the audit as
// the kind of broad inner suppression that silently shadows real
// findings once the module ships. As Step 2 lands, this attribute
// can come off entirely.
#[allow(dead_code)]
pub(crate) mod excerpt_map;
pub(crate) mod handlers;
pub mod ipc;
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

pub use core_plugin::{
    ApplyTransactionResponse, EditorCorePlugin, EditorSnapshot, OpObserver,
    PLUGIN_ID as EDITOR_PLUGIN_ID,
};
