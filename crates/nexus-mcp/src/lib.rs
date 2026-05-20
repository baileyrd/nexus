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
//! Both halves are exposed through the kernel today. The Host client is
//! wrapped by [`core_plugin::McpHostPlugin`] (`com.nexus.mcp.host`),
//! which registers IPC handlers including `connect_server`, `list_servers`,
//! `list_tools`, and `call_tool` — see [`core_plugin::IPC_HANDLERS`] for
//! the authoritative surface. The server half ships as the `nexus-mcp`
//! binary (built from `src/server.rs`) and reaches into the same kernel
//! over `ctx.ipc_call(...)` to call `com.nexus.storage`, `com.nexus.git`,
//! `com.nexus.ai`, and other core plugins.

#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod auth;
mod client;
/// `<forge>/.forge/mcp.toml` parser + the BL-113 / ADR 0027
/// `merge_contributed` API. `pub` so `nexus-bootstrap` can construct
/// `McpServerSpec` values for `merge_contributed`; outside the crate
/// the re-exports at the root are the supported surface.
pub mod config;
pub mod core_plugin;
/// DG-39 / PRD-14 §10 — runtime registry that lets plugins publish
/// tools to the MCP server's exposed surface.
pub mod dynamic_tools;
/// Wire-mirror IPC arg/reply types — the authoritative contract that
/// the schema generator and the shell consume (audit P1-3, #113).
pub mod ipc;
pub mod pool;
mod server;

pub use auth::{AuthError, McpAuth, McpAuthSecret, ResolvedAuth};
pub use client::{McpClient, McpClientError};
pub use config::{
    McpConfigError, McpHostConfig, McpMergeSkip, McpMergeSkipReason, McpServerSpec, McpTransport,
    McpUnregisterError,
};
pub use core_plugin::McpHostPlugin;
pub use dynamic_tools::{DynamicTool, DynamicToolRegistry, RegistryError as ToolRegistryError};
pub use pool::{ConnectionPool, PoolConfig};
pub use server::NexusMcpServer;
