//! Nexus git integration: status, diff, blame, log, staging, commits, branches.
//!
//! Provides [`GitEngine`] for discovering and interacting with git repositories,
//! and [`GitCorePlugin`] to expose git over IPC and publish state-change events
//! to the kernel bus.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod error;
mod engine;
mod lfs;
mod types;
mod auto_commit;
mod worker;
mod handlers;
/// Core plugin registration and IPC handler constants for `com.nexus.git`.
pub mod core_plugin;
/// Wire-mirror IPC arg/reply types — the authoritative contract that
/// the schema generator and the shell consume (audit P1-3, #113).
pub mod ipc;

pub use error::GitError;
pub use engine::GitEngine;
pub use types::*;
pub use auto_commit::{AutoCommitter, AutoCommitResult};
pub use worker::{GitWorker, GitWorkerHandle};
pub use core_plugin::GitCorePlugin;
