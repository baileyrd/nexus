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
                &with_v1_aliases(nexus_mcp::core_plugin::IPC_HANDLERS),
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
