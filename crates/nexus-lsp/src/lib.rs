//! Nexus LSP host.
//!
//! Spawns Language Server Protocol servers from `<forge>/.forge/lsp.toml`,
//! bridges their JSON-RPC stdio streams to the kernel IPC surface
//! (`com.nexus.lsp`), and republishes server-pushed diagnostics on the
//! event bus as `com.nexus.lsp.diagnostics.<path>`.
//!
//! Architecture mirrors [`nexus-mcp`](../nexus_mcp/index.html) — a
//! [`ConnectionPool`] holds at most one [`LspClient`] per configured
//! server, lazily connected on first use and reconnected with
//! exponential backoff on transient failure.
//!
//! The host is a transparent proxy: most requests forward
//! `serde_json::Value` arguments straight to the upstream server and
//! return the raw response. Only [`open_file`] / [`close_file`] /
//! [`change_file`] need protocol awareness, because they translate IPC
//! arguments into the LSP `textDocument/did{Open,Close,Change}` shape
//! and update internal document state so a crashed server can
//! re-synchronise on reconnect.

#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod client;
/// `<forge>/.forge/lsp.toml` parser + the BL-113 / ADR 0027
/// `merge_contributed` API. `pub` so `nexus-bootstrap` can construct
/// `LspServerSpec` values for `merge_contributed`; outside the crate
/// the re-exports at the root are the supported surface.
pub mod config;
pub mod core_plugin;
/// Wire-mirror IPC arg/reply types for the schema generator.
pub mod ipc;
pub mod pool;
mod transport;

pub use client::{LspClient, LspClientError, OpenDocument};
pub use config::{
    LspConfigError, LspHostConfig, LspServerSpec, MergeSkip as LspMergeSkip,
    MergeSkipReason as LspMergeSkipReason,
};
pub use core_plugin::LspCorePlugin;
pub use pool::{ConnectionPool, PoolConfig};
