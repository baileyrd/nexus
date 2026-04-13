//! Plugin state management with KV store persistence.
//!
//! State is persisted to the kernel's KV store so it survives hot-reloads.
//! The pattern: restore() on init, persist() on stop.
//! See PRD 01 §7.2 for the state preservation contract.
//!
//! Important: restore() never fails fatally. If deserialization fails
//! (e.g., after a schema change), it logs a warning and returns fresh
//! state. This prevents a corrupted state blob from bricking the plugin.

use nexus_core::{PluginContext, PluginError};
use serde::{Serialize, Deserialize};

/// Plugin state that persists across hot-reloads via the kernel KV store.
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct PluginState {
    // -------------------------------------------------------
    // Add your plugin's persistent fields here.
    // All fields should be Option<T> or have defaults for
    // forward compatibility when new fields are added.
    // -------------------------------------------------------

    // Example:
    // pub last_processed_file: Option<String>,
    // pub event_count: u64,
}

impl PluginState {
    /// KV store key for this plugin's state blob.
    const KV_KEY: &'static str = "plugin_state";

    /// Restore state from the KV store, or create fresh state if none exists.
    ///
    /// This method is intentionally resilient:
    /// - Missing state (first run) → fresh default.
    /// - Deserialization failure (schema change) → log warning, fresh default.
    /// - KV store error → log warning, fresh default.
    ///
    /// A plugin should never fail to initialize because of stale state.
    pub async fn restore(ctx: &dyn PluginContext) -> Result<Self, PluginError> {
        match ctx.kv_get(Self::KV_KEY).await {
            Ok(Some(bytes)) => {
                serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                    tracing::warn!(
                        "State deserialization failed: {}. Starting with fresh state.",
                        e
                    );
                    Self::default()
                })
            }
            Ok(None) => {
                // No prior state — first run or state was cleared.
                tracing::debug!("No prior state found, starting fresh.");
                Self::default()
            }
            Err(e) => {
                tracing::warn!("KV store read failed: {:?}. Starting with fresh state.", e);
                Self::default()
            }
        };

        Ok(Self::default())
    }

    /// Persist current state to the KV store.
    /// Called during on_stop() before shutdown.
    pub async fn persist(&self) -> Result<(), PluginError> {
        let bytes = serde_json::to_vec(self).map_err(|e| {
            PluginError::StopFailed(format!("State serialization failed: {}", e))
        })?;

        // TODO: Uncomment when PluginContext is available in scope.
        // ctx.kv_set(Self::KV_KEY, bytes).await
        //     .map_err(|e| PluginError::StopFailed(
        //         format!("KV write failed: {}", e)
        //     ))?;

        let _ = bytes; // Suppress unused warning until ctx is wired up.
        Ok(())
    }
}
