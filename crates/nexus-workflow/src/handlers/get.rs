//! `com.nexus.workflow::get` handler — fetch one workflow by name.
//! Lifted out of `core_plugin.rs` by the BL-137 oversized-file
//! decomposition.

use std::sync::Mutex;

use nexus_plugins::PluginError;

use crate::core_plugin::GetWorkflowArgs;
use crate::WorkflowRegistry;

use super::shared::{exec_err, parse, poisoned, to_value};

pub(crate) fn handle(
    registry: &Mutex<WorkflowRegistry>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: GetWorkflowArgs = parse(args, "get")?;
    let reg = registry.lock().map_err(poisoned)?;
    match reg.get(&a.name) {
        Some(w) => to_value(w, "get"),
        None => Err(exec_err(format!("no workflow named '{}'", a.name))),
    }
}
