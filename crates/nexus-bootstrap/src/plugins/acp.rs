//! ACP Host plugin registration.
//!
//! Exposes the protocol-host contribution surface for community-plugin-
//! supplied agent adapters (BL-144 / ADR 0027 Phase 4). No flat-TOML
//! class — the registry starts empty and is populated at plugin-load
//! time by `acp_contribution_wiring::wire_acp_contributions`. Agent-
//! pushed notifications fan out on the kernel bus as
//! `com.nexus.acp.<method-with-dots>`.

use std::sync::Arc;

use anyhow::Result;
use nexus_acp::AcpCorePlugin;
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
                "com.nexus.acp",
                "ACP Host",
                LifecycleFlags {
                    on_init: true,
                    on_start: true,
                    on_stop: true,
                },
                &with_v1_aliases(nexus_acp::core_plugin::IPC_HANDLERS),
            ),
            forge_root,
            Box::new(AcpCorePlugin::new(
                forge_root.to_path_buf(),
                Some(Arc::clone(event_bus)),
            )),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.acp")?;
    Ok(())
}
