//! Nexus git integration: status, diff, blame, log, staging, commits, branches.
//!
//! Provides [`GitEngine`] for discovering and interacting with git repositories.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod error;
mod engine;
mod types;
mod auto_commit;

pub use error::GitError;
pub use engine::GitEngine;
pub use types::*;
pub use auto_commit::{AutoCommitter, AutoCommitResult};
