//! DAP Host plugin registration.
//!
//! Loads `<forge>/.forge/dap.toml`, lazily spawns configured debug
//! adapters, proxies DAP requests over IPC, and republishes adapter-
//! pushed events on the kernel bus as `com.nexus.dap.<event>`. BL-081.

use std::sync::Arc;

use anyhow::Result;
use nexus_dap::DapCorePlugin;
use nexus_kernel::EventBus;
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
                "com.nexus.dap",
                "DAP Host",
                LifecycleFlags {
                    on_init: true,
                    on_start: true,
                    on_stop: true,
                },
                &with_v1_aliases(nexus_dap::core_plugin::IPC_HANDLERS),
            ),
            forge_root,
            Box::new(DapCorePlugin::new(
                forge_root.to_path_buf(),
                Some(Arc::clone(event_bus)),
            )),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.dap")?;
    Ok(())
}
