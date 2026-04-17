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

mod adhoc;
mod ansi;
mod buffer;
mod compound;
mod core_plugin;
mod env;
mod error;
mod job_object;
mod lines;
mod manager;
mod persist;
mod precmd;
mod procmgr;
mod profile;
mod saved;
mod server;
mod session;
mod shell;
mod urls;

pub use adhoc::{AdHocRecord, AdHocStatus, SqliteAdHocStore};
pub use ansi::strip_ansi;
pub use buffer::OutputBuffer;
pub use core_plugin::{
    CreateSessionArgs, CreateSessionResponse, PumpArgs, PumpResponse, ReadOutputArgs,
    SearchOutputArgs, SendInputArgs, SendRawInputArgs, SessionIdArgs, TerminalCorePlugin,
    WaitForPatternArgs, WaitForPatternResponse, HANDLER_CLOSE_SESSION, HANDLER_CREATE_SESSION,
    HANDLER_GET_SESSION_INFO, HANDLER_LIST_SESSIONS, HANDLER_PUMP, HANDLER_READ_OUTPUT,
    HANDLER_SEARCH_OUTPUT, HANDLER_SEND_INPUT, HANDLER_SEND_RAW_INPUT, HANDLER_WAIT_FOR_PATTERN,
    PLUGIN_ID,
};
pub use compound::{
    execute_chain, parse_command_chain, requires_single_shell, ChainOutcome, CommandStep,
    Operator, SkipReason, StepOutcome,
};
pub use env::{
    interpolate_env, is_secret_key, mask_secrets, parse_env_file, parse_env_text, resolve_env,
    REDACTED,
};
pub use error::TerminalError;
pub use job_object::JobObject;
pub use lines::{Line, LineBuffer};
pub use manager::{SessionManager, DEFAULT_MAX_SESSIONS};
pub use persist::{SessionMetadata, SqliteSessionStore};
pub use precmd::{
    run_pre_commands, PreCommandOptions, PreCommandOutcome, DEFAULT_STEP_TIMEOUT,
};
pub use procmgr::{
    ManagedConfig, ManagedProcess, ManagedState, TransitionError,
    DEFAULT_AUTO_RESTART_BACKOFF_MS, DEFAULT_PRE_COMMAND_TIMEOUT,
};
pub use profile::{
    profile_path_for_shell, profile_source_command, profile_source_command_for_path,
    supports_profile_sourcing,
};
pub use saved::{
    promote_adhoc_to_saved, slugify, PromoteOptions, SavedCommand, SqliteSavedCommandStore,
    DEFAULT_AUTO_RESTART_DELAY_MS, DEFAULT_ICON,
};
pub use server::{
    InMemoryTerminalServer, OutputLine, ServerSpawnConfig, SessionInfo, TerminalEvent,
    TerminalServer,
};
pub use session::{ProcessState, Session, SessionConfig, SessionId, Signal};
pub use shell::{detect_default_shell, ShellSpec};
pub use urls::{detect_urls, resolve_url, UrlKind, UrlMatch};
