//! `com.nexus.workflow::list` handler — return every loaded workflow
//! as a JSON array. Lifted out of `core_plugin.rs` by the BL-137
//! oversized-file decomposition.

use std::sync::Mutex;

use nexus_plugins::PluginError;

use crate::WorkflowRegistry;

use super::shared::{poisoned, to_value};

pub(crate) fn handle(
    registry: &Mutex<WorkflowRegistry>,
) -> Result<serde_json::Value, PluginError> {
    let reg = registry.lock().map_err(poisoned)?;
    let workflows: Vec<_> = reg.iter().map(|(_, w)| w.clone()).collect();
    to_value(&workflows, "list")
}
