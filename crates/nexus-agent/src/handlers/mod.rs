//! Per-handler IPC modules for the agent core plugin.
//!
//! `core_plugin.rs` keeps the `CorePlugin` trait impl and dispatches
//! into these by handler id. Cross-cutting helpers live in `shared`.

pub(crate) mod ask;
pub(crate) mod changes;
pub(crate) mod checkpoint;
pub(crate) mod custom;
pub(crate) mod delegate;
pub(crate) mod history;
pub(crate) mod list_tools;
pub(crate) mod memory;
pub(crate) mod plan;
pub(crate) mod round;
pub(crate) mod search_transcripts;
pub(crate) mod session;
pub(crate) mod shared;
