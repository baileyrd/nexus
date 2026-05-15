//! MCP Host plugin registration.
//!
//! Loads mcp.toml, lazily connects to external MCP servers, exposes
//! `list_tools` / `call_tool` / `list_resources` / `list_prompts`
//! over IPC so any plugin or invoker can reach external tools without
//! linking the rmcp crate directly.

use std::sync::Arc;

use anyhow::Result;
use nexus_kernel::EventBus;
use nexus_mcp::McpHostPlugin;
use nexus_plugins::PluginLoader;

use super::{core_manifest_with_ipc, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.mcp.host",
                "MCP Host",
                LifecycleFlags {
                    on_init: true,
                    on_start: true,
                    on_stop: true,
                },
                &with_v1_aliases(&[
                    (
                        "list_servers",
                        nexus_mcp::core_plugin::HANDLER_LIST_SERVERS,
                    ),
                    ("list_tools", nexus_mcp::core_plugin::HANDLER_LIST_TOOLS),
                    ("call_tool", nexus_mcp::core_plugin::HANDLER_CALL_TOOL),
                    (
                        "list_resources",
                        nexus_mcp::core_plugin::HANDLER_LIST_RESOURCES,
                    ),
                    (
                        "list_prompts",
                        nexus_mcp::core_plugin::HANDLER_LIST_PROMPTS,
                    ),
                    ("connect", nexus_mcp::core_plugin::HANDLER_CONNECT),
                    ("disconnect", nexus_mcp::core_plugin::HANDLER_DISCONNECT),
                ]),
            ),
            forge_root,
            Box::new(McpHostPlugin::new(
                forge_root.to_path_buf(),
                Some(Arc::clone(event_bus)),
            )),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.mcp.host")?;
    Ok(())
}
