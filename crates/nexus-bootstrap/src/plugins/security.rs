//! Security plugin registration.
//!
//! Registered first so audit events are available before other plugins emit.

use std::sync::Arc;

use anyhow::Result;
use nexus_kernel::EventBus;
use nexus_plugins::PluginLoader;
use nexus_security::SecurityCorePlugin;

use super::{core_manifest_with_ipc, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.security",
                "Security",
                LifecycleFlags {
                    on_init: true,
                    on_start: true,
                    on_stop: true,
                },
                &with_v1_aliases(nexus_security::core_plugin::IPC_HANDLERS),
            ),
            forge_root,
            Box::new(SecurityCorePlugin::new(Some(Arc::clone(event_bus)))),
        )
        // Security is critical: capability gates and audit emission
        // depend on it. A degraded boot without security would either
        // fail closed on every cap-gated IPC call (best case — every
        // editor save errors out) or fall back to permissive behavior
        // (worst case — caps don't enforce). Use `or_critical` so a
        // lifecycle hang aborts boot instead of either outcome.
        .or_critical("com.nexus.security")?;
    Ok(())
}
