//! Nexus remote-forge JSON-RPC server (BL-140 Phase 1).
//!
//! Exposes the kernel's IPC surface and event bus over a line-delimited
//! JSON-RPC 2.0 stdio stream so a local frontend (CLI, shell, Tauri host)
//! can drive a headless Nexus instance running somewhere else. The
//! binary entry point is `nexus serve --stdio`; SSH transport and
//! `ssh://user@host/path` forge URIs land in Phase 2.
//!
//! # Wire framing
//!
//! Newline-delimited JSON-RPC 2.0 — one message per line, no
//! `Content-Length` header. Same shape as
//! [`nexus_acp`](../nexus_acp/index.html); the framing is duplicated
//! here on purpose (per the BL-140 design call) so the two crates can
//! evolve their surfaces independently — ACP's allow-list narrowness
//! shouldn't constrain a remote-forge proxy that exposes the whole IPC
//! tree.
//!
//! # Exposed methods
//!
//! | JSON-RPC method | Behaviour |
//! |---|---|
//! | `ipc_call` | Routed verbatim to [`PluginContext::ipc_call`]. No allow-list — the *point* of remote forge is full IPC surface access, with trust delegated to the transport layer (SSH in Phase 2). |
//! | `event_subscribe` | Registers a long-lived subscription. Matching events stream back as server-pushed `event` notifications carrying the supplied `subscription_id`. |
//! | `event_unsubscribe` | Cancels one subscription by id. |
//!
//! Unknown methods return `-32601`. Invalid params return `-32602`.
//! Underlying `ipc_call` failures return `-32000`.

#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod server;
pub mod transport;

pub use server::{RemoteServer, RemoteServerError};
pub use transport::{
    JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse,
    TransportError, MAX_LINE_BYTES,
};
