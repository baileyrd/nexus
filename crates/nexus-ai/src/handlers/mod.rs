//! Per-handler IPC modules for the AI core plugin.
//!
//! `core_plugin.rs` keeps the `CorePlugin` trait impl, the
//! `AiCorePlugin` struct + lifecycle hooks, and dispatches into these
//! modules by handler id. Cross-cutting helpers (provider routing,
//! streaming envelope, tool-loop adapter, error converters, etc.) live
//! in `shared`.

pub(crate) mod activity;
pub(crate) mod ask;
pub(crate) mod config;
pub(crate) mod enrich;
pub(crate) mod entity;
pub(crate) mod index;
pub(crate) mod propose;
pub(crate) mod search;
pub(crate) mod session;
pub(crate) mod shared;
pub(crate) mod stream_ask;
pub(crate) mod stream_chat;
