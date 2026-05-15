//! AI runtime plugin registration.
//!
//! BL-134 / ADR 0028 Phase 1 — `com.nexus.ai.runtime` task
//! scheduler + worker pool. Registered after notifications so
//! any republished `AiEvent::Failed` can route through the
//! notifications subsystem once Phase 6 wires the router.

use std::sync::Arc;

use anyhow::Result;
use nexus_ai_runtime::core_plugin::AiRuntimeCorePlugin;
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
                "com.nexus.ai.runtime",
                "AiRuntime",
                LifecycleFlags::NONE,
                &with_v1_aliases(&[
                    ("submit", nexus_ai_runtime::core_plugin::HANDLER_SUBMIT),
                    ("cancel", nexus_ai_runtime::core_plugin::HANDLER_CANCEL),
                    ("pause", nexus_ai_runtime::core_plugin::HANDLER_PAUSE),
                    ("resume", nexus_ai_runtime::core_plugin::HANDLER_RESUME),
                    ("get", nexus_ai_runtime::core_plugin::HANDLER_GET),
                    ("list", nexus_ai_runtime::core_plugin::HANDLER_LIST),
                    ("events", nexus_ai_runtime::core_plugin::HANDLER_EVENTS),
                    (
                        "pool_stats",
                        nexus_ai_runtime::core_plugin::HANDLER_POOL_STATS,
                    ),
                ]),
            ),
            forge_root,
            Box::new(AiRuntimeCorePlugin::new()),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.ai.runtime")?;
    Ok(())
}
