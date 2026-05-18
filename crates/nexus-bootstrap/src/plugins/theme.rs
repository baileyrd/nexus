//! Theme plugin registration.
//!
//! Theme engine — registered as a core plugin so (a) other plugins can
//! call `ipc_call("com.nexus.theme", …)` and subscribe to
//! `com.nexus.theme.changed` events, and (b) the Tauri shell's theme
//! commands are thin adapters over kernel IPC rather than owning
//! engine state directly. See PRD-07.

use std::sync::Arc;

use anyhow::Result;
use nexus_kernel::EventBus;
use nexus_plugins::PluginLoader;
use nexus_theme::ThemeCorePlugin;

use super::{core_manifest_with_ipc, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.theme",
                "Theme",
                LifecycleFlags::NONE,
                &with_v1_aliases(nexus_theme::core_plugin::IPC_HANDLERS),
            ),
            forge_root,
            Box::new(ThemeCorePlugin::with_builtins(Some(Arc::clone(event_bus)))),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.theme")?;
    Ok(())
}
