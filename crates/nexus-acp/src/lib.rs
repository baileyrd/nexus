//! Nexus ACP host + inbound server.
//!
//! Two complementary roles live in one crate:
//!
//! 1. **Host (BL-144 / ADR 0027 Phase 4)** — outbound. The
//!    [`AcpCorePlugin`] core plugin spawns ACP-speaking agent
//!    sub-processes declared by community plugins through the
//!    `[[registrations.protocol_hosts.acp]]` manifest contribution
//!    point, proxies request/response traffic over IPC
//!    (`com.nexus.acp`), and republishes agent-pushed notifications on
//!    the kernel bus as `com.nexus.acp.<method-with-dots>`. Mirrors
//!    [`nexus-lsp`](../nexus_lsp/index.html) layer for layer — there is
//!    intentionally no `acp.toml` flat-TOML loader (ADR 0027 §Phase 4:
//!    ACP lands greenfield under the contribution model).
//! 2. **Server (BL-145 / Hermes Feature 7)** — inbound. [`AcpServer`]
//!    is a line-delimited JSON-RPC 2.0 stdio surface that exposes a
//!    fixed subset of Nexus's `com.nexus.agent` IPC verbs to external
//!    Hermes-compatible clients. Started by the `nexus acp serve`
//!    CLI binary. Pure proxy — no kernel-context borrow; all calls
//!    route through [`nexus_kernel::PluginContext::ipc_call`].
//!
//! # Wire framing
//!
//! ACP uses **newline-delimited JSON** (one JSON-RPC 2.0 message per
//! line) rather than LSP's `Content-Length:` header framing. This
//! matches the Hermes Feature-7 wire shape + most JSON-RPC tooling
//! defaults (jsonrpc-cli, jq pipelines, …) and keeps the transport
//! debuggable from a terminal.

#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod client;
/// In-memory adapter registry + BL-113 / ADR 0027 contribution API.
/// `pub` so `nexus-bootstrap` can construct [`AcpAdapterSpec`] values
/// for `register_contributed`; outside the crate the re-exports at the
/// root are the supported surface.
pub mod config;
pub mod core_plugin;
/// Wire-mirror IPC arg/reply types for the schema generator.
pub mod ipc;
pub mod pool;
pub mod server;
mod transport;

pub use client::{AcpClient, AcpClientError};
pub use config::{
    AcpAdapterSpec, AcpConfigError, AcpHostConfig, MergeSkip as AcpMergeSkip,
    MergeSkipReason as AcpMergeSkipReason, UnregisterError as AcpUnregisterError,
};
pub use core_plugin::AcpCorePlugin;
pub use pool::{ConnectionPool, PoolConfig};
pub use server::{AcpServer, AcpServerError};
