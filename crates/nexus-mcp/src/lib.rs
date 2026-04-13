//! Nexus MCP server: exposes forge operations as MCP tools.

#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod server;

pub use server::NexusMcpServer;
