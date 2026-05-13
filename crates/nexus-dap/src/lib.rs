//! Nexus DAP host.
//!
//! Spawns Debug Adapter Protocol adapters from `<forge>/.forge/dap.toml`,
//! bridges their stdio JSON envelope to the kernel IPC surface
//! (`com.nexus.dap`), and republishes adapter-pushed events on the
//! kernel event bus as `com.nexus.dap.<event>`.
//!
//! Architecture mirrors [`nexus-lsp`](../nexus_lsp/index.html) almost
//! 1:1 — same Content-Length JSON framing, same per-adapter
//! [`ConnectionPool`] with lazy connect + reconnect-with-backoff. The
//! only protocol differences (DAP's `type`-tagged envelope, the
//! request/response/event triplet, and the `seq` correlation id
//! instead of JSON-RPC `id`) are contained in [`protocol`].

#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod client;
pub mod config;
pub mod core_plugin;
pub mod ipc;
pub mod pool;
pub mod protocol;
pub mod transport;

pub use client::{AdapterCapabilities, DapClient, DapClientError, SourceBreakpointSpec};
pub use config::{DapAdapterSpec, DapConfigError, DapHostConfig};
pub use core_plugin::DapCorePlugin;
pub use pool::{ConnectionPool, PoolConfig};
pub use protocol::{ProtocolEvent, ProtocolMessage, ProtocolRequest, ProtocolResponse};
