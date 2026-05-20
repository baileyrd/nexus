//! Agent plugin registration.
//!
//! PRD-15 scaffold. Thin dispatch surface over
//! `nexus-agent::{LlmAgent, PlanExecutor}`; bridges to `com.nexus.ai`
//! for planning and to arbitrary plugins for tool calls via the
//! `KernelPluginContext` wired in `lib.rs::build`.

use std::sync::Arc;

use anyhow::Result;
use nexus_agent::AgentCorePlugin;
use nexus_kernel::EventBus;
use nexus_plugins::PluginLoader;

use super::{core_manifest_with_ipc_and_deps, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    loader
        .register_core(
            core_manifest_with_ipc_and_deps(
                "com.nexus.agent",
                "Agent",
                // BL-121 — on_init opens the transcript-search FTS
                // index. on_start / on_stop stay as no-ops.
                LifecycleFlags {
                    on_init: true,
                    on_start: false,
                    on_stop: false,
                },
                &with_v1_aliases(nexus_agent::core_plugin::IPC_HANDLERS),
                nexus_agent::core_plugin::MANIFEST_DEPS,
            ),
            forge_root,
            Box::new(AgentCorePlugin::new_with_forge(forge_root.to_path_buf())),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.agent")?;

    // DG-32 — seed the agent-tool registry's process-global catalogue
    // once the agent core plugin is registered. Read by
    // `com.nexus.agent::list_tools` and by `nexus tool list`.
    nexus_agent::seed_default_tools();
    Ok(())
}
