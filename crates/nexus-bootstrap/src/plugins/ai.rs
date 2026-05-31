//! AI plugin registration.

use std::sync::Arc;

use anyhow::Result;
use nexus_ai::AiCorePlugin;
use nexus_kernel::EventBus;
use nexus_plugins::PluginLoader;

use super::{
    core_manifest_with_ipc_and_deps, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt,
};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    loader
        .register_core(
            core_manifest_with_ipc_and_deps(
                "com.nexus.ai",
                "AI",
                LifecycleFlags {
                    on_init: true,
                    // BL-041 — gracefully tear down the background
                    // indexing daemon on shutdown. (`on_start` stays
                    // false; the daemon is spawned from
                    // `wire_context` because that's the first hook
                    // with the kernel context in hand.)
                    on_stop: true,
                    ..LifecycleFlags::NONE
                },
                &with_v1_aliases(nexus_ai::core_plugin::IPC_HANDLERS),
                nexus_ai::core_plugin::MANIFEST_DEPS,
            ),
            forge_root,
            Box::new(AiCorePlugin::new()),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.ai")?;
    Ok(())
}
