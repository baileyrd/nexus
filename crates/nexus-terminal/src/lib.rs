//! Nexus terminal & process manager — Phase A: PTY primitive.
//!
//! # PRD-09 scope
//!
//! [PRD-09] specifies a terminal subsystem spanning PTY sessions, a ring-buffer
//! output capture, a process lifecycle state machine, signal handling, URL
//! detection, memory monitoring, environment variable resolution, compound
//! command splitting, an ad-hoc command system, a programmable API, AI
//! integration, and a SQLite-backed persistence schema.
//!
//! This crate today ships the **foundation**: spawning a PTY, writing to it,
//! reading from it, resizing it, and killing the child cleanly. Every other
//! PRD-09 section composes on top of this surface without requiring changes
//! to it.
//!
//! [PRD-09]: ../../docs/PRDs/09-terminal-process-manager.md
//!
//! # Microkernel fit
//!
//! `nexus-terminal` is an **invoker-local library**, not a core plugin yet —
//! mirroring the positioning of `nexus-git` (PRD-11) and the MCP Host
//! client. Nothing in the kernel event bus or plugin IPC calls the terminal
//! today; when a consumer appears, a `com.nexus.terminal` core plugin can
//! wrap [`Session`] in dispatch handlers without touching this module.
//!
//! The only shared-trait dependency is the workspace's capability enum via
//! future host functions — and none land here. Keeping the crate a pure
//! library means the kernel does not have to know about PTYs at all.
//!
//! # Threading model
//!
//! `portable_pty::Child` and master PTY handles are `Send` but not `Sync`.
//! [`Session`] therefore owns its handles behind `Mutex`es and exposes
//! blocking read/write operations. Async callers should route each call
//! through `tokio::task::spawn_blocking` (or use the future `SessionHandle`
//! worker pattern — same shape as `GitWorkerHandle`).
//!
//! # Example
//!
//! ```ignore
//! use nexus_terminal::{Session, SessionConfig};
//!
//! let mut session = Session::spawn(SessionConfig::default())?;
//! session.write(b"echo hello\n")?;
//! // Poll for output with a short timeout.
//! let mut buf = [0u8; 4096];
//! let n = session.read(&mut buf, std::time::Duration::from_millis(500))?;
//! assert!(buf[..n].windows(5).any(|w| w == b"hello"));
//! session.kill()?;
//! ```

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod ansi;
mod buffer;
mod error;
mod lines;
mod manager;
mod session;
mod shell;

pub use ansi::strip_ansi;
pub use buffer::OutputBuffer;
pub use error::TerminalError;
pub use lines::{Line, LineBuffer};
pub use manager::{SessionManager, DEFAULT_MAX_SESSIONS};
pub use session::{Session, SessionConfig, SessionId, Signal};
pub use shell::{detect_default_shell, ShellSpec};
