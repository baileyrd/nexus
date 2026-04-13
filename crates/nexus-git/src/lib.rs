//! Nexus git integration: read-only status, diff, blame, and log via libgit2.
//!
//! Provides [`GitEngine`] for discovering and querying git repositories.
//! All operations are read-only (Level 1 passive awareness).

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod error;
mod engine;
mod types;

pub use error::GitError;
pub use engine::GitEngine;
pub use types::*;
