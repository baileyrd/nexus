//! Core plugin wrapper for the AI engine.
//!
//! Registers as `com.nexus.ai` and participates in the plugin lifecycle.
//! Detects the configured AI provider on `on_init` and exposes AI operations
//! to other plugins via IPC dispatch.
//!
//! # IPC
//!
//! IPC handler IDs are reserved but not yet wired — `dispatch` returns a
//! descriptive error until async dispatch is plumbed through the plugin system.

use nexus_plugins::{CorePlugin, PluginError};

use crate::config::{detect_provider, AiConfig};

/// Reverse-DNS identifier for this plugin.
const PLUGIN_ID: &str = "com.nexus.ai";

/// Core plugin for AI integration.
///
/// Detects the AI provider from environment variables on `on_init`.
/// IPC dispatch is reserved for future async-capable handler wiring.
pub struct AiCorePlugin {
    config: Option<AiConfig>,
}

impl AiCorePlugin {
    /// Create a new (unstarted) plugin.
    #[must_use]
    pub fn new() -> Self {
        Self { config: None }
    }

    /// Return the detected AI configuration, available after `on_init`.
    #[must_use]
    pub fn config(&self) -> Option<&AiConfig> {
        self.config.as_ref()
    }
}

impl Default for AiCorePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl CorePlugin for AiCorePlugin {
    /// Detect the AI provider from environment variables.
    ///
    /// Succeeds even when no provider is found (AI is optional).
    fn on_init(&mut self) -> Result<(), PluginError> {
        self.config = detect_provider();
        if let Some(cfg) = &self.config {
            tracing::debug!(
                plugin_id = PLUGIN_ID,
                provider = %cfg.provider,
                "AI provider detected"
            );
        } else {
            tracing::debug!(
                plugin_id = PLUGIN_ID,
                "no AI provider detected; AI features disabled"
            );
        }
        Ok(())
    }

    /// Dispatch an IPC handler call.
    ///
    /// Handler IDs are reserved for future AI IPC commands.  Returns a
    /// descriptive error until async dispatch is wired through the plugin system.
    fn dispatch(
        &mut self,
        handler_id: u32,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!(
                "handler {handler_id}: AI IPC not yet implemented \
                 (pending async dispatch support)"
            ),
        })
    }
}
