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

use super::{core_manifest_with_ipc, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.agent",
                "Agent",
                // BL-121 — on_init opens the transcript-search FTS
                // index. on_start / on_stop stay as no-ops.
                LifecycleFlags {
                    on_init: true,
                    on_start: false,
                    on_stop: false,
                },
                &with_v1_aliases(&[
                    ("plan", nexus_agent::HANDLER_PLAN),
                    ("history_list", nexus_agent::HANDLER_HISTORY_LIST),
                    ("history_get", nexus_agent::HANDLER_HISTORY_GET),
                    ("history_delete", nexus_agent::HANDLER_HISTORY_DELETE),
                    ("list_archetypes", nexus_agent::HANDLER_LIST_ARCHETYPES),
                    // ADR 0024 Phase 2a — agent session tool-loop.
                    ("session_run", nexus_agent::core_plugin::HANDLER_SESSION_RUN),
                    ("session_list", nexus_agent::core_plugin::HANDLER_SESSION_LIST),
                    ("session_get", nexus_agent::core_plugin::HANDLER_SESSION_GET),
                    ("session_delete", nexus_agent::core_plugin::HANDLER_SESSION_DELETE),
                    // ADR 0024 Phase 2b — caller-side approval reply.
                    ("round_decide", nexus_agent::core_plugin::HANDLER_ROUND_DECIDE),
                    // DG-32 (PRD-15 §4) — agent tool registry discovery.
                    ("list_tools", nexus_agent::HANDLER_LIST_TOOLS),
                    // DG-36 (PRD-15 §9) — custom .agent.toml manifests.
                    ("list_custom", nexus_agent::HANDLER_LIST_CUSTOM),
                    // DG-33 (PRD-15 §5) — agent-scoped persistent memory.
                    ("memory_record", nexus_agent::HANDLER_MEMORY_RECORD),
                    ("memory_query", nexus_agent::HANDLER_MEMORY_QUERY),
                    ("memory_prune", nexus_agent::HANDLER_MEMORY_PRUNE),
                    ("memory_export", nexus_agent::HANDLER_MEMORY_EXPORT),
                    // DG-37 (PRD-15 §10) — agent-to-agent delegation.
                    ("delegate", nexus_agent::HANDLER_DELEGATE),
                    // BL-121 — FTS5-backed transcript search over
                    // `.forge/agents/*/history.jsonl`.
                    (
                        "search_transcripts",
                        nexus_agent::core_plugin::HANDLER_SEARCH_TRANSCRIPTS,
                    ),
                ]),
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
