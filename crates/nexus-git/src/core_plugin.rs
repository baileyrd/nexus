//! Core plugin wrapper for the git engine.
//!
//! Registers as `com.nexus.git` and participates in the plugin lifecycle.
//! Because `git2::Repository` is not `Send + Sync`, the engine is created
//! per-dispatch rather than held as a field.
//!
//! # IPC
//!
//! IPC handler IDs are reserved but not yet wired — `dispatch` returns a
//! descriptive error until the git types gain `serde::Serialize` derives.

use std::path::PathBuf;

use nexus_plugins::{CorePlugin, PluginError};

use crate::GitEngine;

/// Reverse-DNS identifier for this plugin.
const PLUGIN_ID: &str = "com.nexus.git";

/// Core plugin for git integration.
///
/// Verifies on `on_init` that the forge root is inside a git repository.
/// IPC dispatch is reserved for when git types gain `serde::Serialize`.
pub struct GitCorePlugin {
    forge_root: PathBuf,
    /// Set to `true` after `on_init` confirms a repository is present.
    repo_confirmed: bool,
}

impl GitCorePlugin {
    /// Create a new (unstarted) plugin for the forge at `forge_root`.
    pub fn new(forge_root: PathBuf) -> Self {
        Self {
            forge_root,
            repo_confirmed: false,
        }
    }

    /// Return `true` if a git repository was found during `on_init`.
    #[must_use]
    pub fn is_repo(&self) -> bool {
        self.repo_confirmed
    }
}

impl CorePlugin for GitCorePlugin {
    /// Verify that `forge_root` is inside a git repository.
    ///
    /// Succeeds even when no repository is found (git is optional); logs a
    /// debug message so operators can distinguish the two cases.
    fn on_init(&mut self) -> Result<(), PluginError> {
        match GitEngine::open(&self.forge_root) {
            Ok(_) => {
                self.repo_confirmed = true;
                tracing::debug!(
                    plugin_id = PLUGIN_ID,
                    forge_root = %self.forge_root.display(),
                    "git repository confirmed"
                );
            }
            Err(e) => {
                tracing::debug!(
                    plugin_id = PLUGIN_ID,
                    forge_root = %self.forge_root.display(),
                    error = %e,
                    "no git repository found; git features disabled"
                );
            }
        }
        Ok(())
    }

    /// Dispatch an IPC handler call.
    ///
    /// Handler IDs are reserved for future git IPC commands.  Returns a
    /// descriptive error until git types gain `serde::Serialize` derives.
    fn dispatch(
        &mut self,
        handler_id: u32,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!(
                "handler {handler_id}: git IPC not yet implemented \
                 (pending serde::Serialize on git types)"
            ),
        })
    }
}
