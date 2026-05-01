//! Nexus MCP server + Host client.
//!
//! Two halves of the same protocol:
//!
//! - [`NexusMcpServer`] (in `server`) exposes Nexus forge operations as MCP
//!   tools to external AI clients (Claude Desktop, Cursor, Cline, …).
//! - [`McpClient`] + [`McpHostConfig`] (in `client` / `config`) let Nexus
//!   itself connect to external MCP servers listed in
//!   `<forge>/.forge/mcp.toml` as a Host, mirroring Claude Desktop's
//!   `mcp.json` pattern.
//!
//! Both halves are invoker-local libraries — no IPC surface, no core
//! plugin wrapper — because no kernel or plugin consumer calls them today.
//! The natural next step, if one appears, is a `com.nexus.mcp.host` core
//! plugin that exposes `connect_server` / `list_tools` / `call_tool` IPC
//! handlers; it would layer on top of this module without requiring any
//! changes here.

#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod auth;
mod client;
mod config;
pub mod core_plugin;
/// Wire-mirror IPC arg/reply types — the authoritative contract that
/// the schema generator and the shell consume (audit P1-3, #113).
pub mod ipc;
pub mod pool;
mod server;

pub use auth::{AuthError, McpAuth, McpAuthSecret, ResolvedAuth};
pub use client::{McpClient, McpClientError};
pub use config::{McpConfigError, McpHostConfig, McpServerSpec, McpTransport};
pub use core_plugin::McpHostPlugin;
pub use pool::{ConnectionPool, PoolConfig};
pub use server::NexusMcpServer;
