//! Memory plugin registration.
//!
//! Wires `nexus-memory`'s `com.nexus.memory` CorePlugin into the boot path.
//! The engine persists to `<forge>/.forge/memory/memory.db` and is exposed over
//! kernel IPC so the agent loop, shell, and MCP clients reach it through the one
//! `ipc_call` path. Registered right after storage — it owns its own SQLite
//! store and declares no inter-plugin dependencies.

use std::sync::Arc;

use anyhow::Result;
use nexus_kernel::EventBus;
use nexus_memory::MemoryCorePlugin;
use nexus_plugins::PluginLoader;

use super::{core_manifest_with_ipc, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt};

pub(crate) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    let plugin = MemoryCorePlugin::open(forge_root)?;
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.memory",
                "Memory",
                LifecycleFlags::NONE,
                &with_v1_aliases(nexus_memory::core_plugin::IPC_HANDLERS),
            ),
            forge_root,
            Box::new(plugin),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.memory")?;
    Ok(())
}
