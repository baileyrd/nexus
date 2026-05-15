//! `com.nexus.workflow::reload` handler — re-scan the forge's
//! `.workflows/` directory and replace the in-memory registry. Lifted
//! out of `core_plugin.rs` by the BL-137 oversized-file decomposition.

use std::path::Path;
use std::sync::Mutex;

use nexus_plugins::PluginError;

use crate::WorkflowRegistry;

use super::shared::poisoned;

pub(crate) fn handle(
    root: &Path,
    registry: &Mutex<WorkflowRegistry>,
) -> Result<serde_json::Value, PluginError> {
    let reloaded = WorkflowRegistry::load(root).unwrap_or_else(|_| WorkflowRegistry::empty());
    let len = reloaded.len();
    *registry.lock().map_err(poisoned)? = reloaded;
    Ok(serde_json::json!({ "loaded": len }))
}
