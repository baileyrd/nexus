//! DG-32 — `list_tools` handler. Returns the agent tool registry's
//! seeded catalogue, optionally filtered by held capabilities.

use nexus_plugins::PluginError;
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use super::shared::{exec_err, parse, to_value};

/// Args for `com.nexus.agent::list_tools` (handler id 18).
#[derive(Deserialize, Serialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ListToolsArgs {
    /// Optional capability filter.
    #[serde(default)]
    pub capabilities: Option<Vec<String>>,
}

pub(crate) fn handle_list_tools(
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: ListToolsArgs = if args.is_null() {
        ListToolsArgs { capabilities: None }
    } else {
        parse(args, "list_tools")?
    };
    let registry = crate::AgentToolRegistry::global();
    let specs = match a.capabilities {
        None => registry.list_all(),
        Some(ids) => {
            let mut held = Vec::with_capacity(ids.len());
            for id in ids {
                let cap = crate::Capability::from_str(&id).ok_or_else(|| {
                    exec_err(format!("list_tools: unknown capability id '{id}'"))
                })?;
                held.push(cap);
            }
            registry.list_for_agent(&held)
        }
    };
    let mut sorted = specs;
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    to_value(&sorted, "list_tools")
}
