//! Nexus git integration: status, diff, blame, log, staging, commits, branches.
//!
//! Provides [`GitEngine`] for discovering and interacting with git repositories,
//! and [`GitCorePlugin`] to expose git over IPC and publish state-change events
//! to the kernel bus.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod auto_commit;
/// Core plugin registration and IPC handler constants for `com.nexus.git`.
pub mod core_plugin;
mod engine;
mod error;
mod handlers;
/// Wire-mirror IPC arg/reply types — the authoritative contract that
/// the schema generator and the shell consume (audit P1-3, #113).
pub mod ipc;
mod lfs;
mod types;
mod worker;

pub use auto_commit::{AutoCommitResult, AutoCommitter};
pub use core_plugin::GitCorePlugin;
pub use engine::GitEngine;
pub use error::GitError;
pub use types::*;
pub use worker::{GitWorker, GitWorkerHandle};
