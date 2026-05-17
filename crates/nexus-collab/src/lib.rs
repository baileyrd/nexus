//! Nexus live-collaboration network transport (BL-143 Phase 1).
//!
//! Provides a WebSocket relay that ferries CRDT-op envelopes (the
//! `com.nexus.editor.ops.<relpath>` topic family shipped by ADR 0026
//! Phase 3) and presence updates between peers running on different
//! machines. The relay itself is topic-agnostic — it routes opaque
//! `payload` JSON tagged with a kernel-bus topic — so it stays stable
//! across consumer evolution.
//!
//! This crate is the relay server only. The matching kernel-side
//! client bridge that subscribes to the local event bus, ships
//! envelopes to the relay, and re-publishes inbound envelopes back to
//! the bus lands under BL-143.2. CLI verbs (`nexus collab serve` /
//! `nexus collab join`) land under BL-143.4.
//!
//! # Quick start
//!
//! ```no_run
//! use std::sync::Arc;
//! use nexus_collab::{RelayServer, Token};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let token = Token::new("hunter2")?;
//! let server = Arc::new(RelayServer::new(token));
//! let listener = tokio::net::TcpListener::bind("127.0.0.1:7700").await?;
//! server.serve_listener(listener).await?;
//! # Ok(())
//! # }
//! ```

#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod auth;
pub mod protocol;
pub mod server;

pub use auth::{Token, TokenError};
pub use protocol::{
    ClientMessage, PeerInfo, ServerMessage, ERR_AUTH, ERR_BAD_FRAME, ERR_HANDSHAKE,
};
pub use server::{RelayServer, RelayServerError};
