//! Workflow plugin registration.
//!
//! PRD-16 scaffold. Read-mostly surface over `.workflows/` TOML files.
//! Library stays kernel-free; this plugin is the only integration point.

use std::sync::Arc;

use anyhow::Result;
use nexus_kernel::EventBus;
use nexus_plugins::PluginLoader;
use nexus_workflow::WorkflowCorePlugin;

use crate::{load_digest_config, load_webhook_config};

use super::{core_manifest_with_ipc, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    let workflows_dir = forge_root.join(".workflows");
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.workflow",
                "Workflow",
                LifecycleFlags::NONE,
                &with_v1_aliases(&[
                    ("list", nexus_workflow::HANDLER_LIST),
                    ("get", nexus_workflow::HANDLER_GET),
                    ("reload", nexus_workflow::HANDLER_RELOAD),
                    ("validate", nexus_workflow::HANDLER_VALIDATE),
                    ("run", nexus_workflow::HANDLER_RUN),
                    ("run_digest", nexus_workflow::HANDLER_RUN_DIGEST),
                    // FU-7 — live config push. Lets the shell flip
                    // [digests].enabled / cron strings without
                    // restarting the kernel.
                    (
                        "set_digest_config",
                        nexus_workflow::HANDLER_SET_DIGEST_CONFIG,
                    ),
                    // BL-028f — built-in templates library.
                    (
                        "templates_list",
                        nexus_workflow::core_plugin::HANDLER_TEMPLATES_LIST,
                    ),
                    (
                        "templates_get",
                        nexus_workflow::core_plugin::HANDLER_TEMPLATES_GET,
                    ),
                    (
                        "templates_init",
                        nexus_workflow::core_plugin::HANDLER_TEMPLATES_INIT,
                    ),
                    // BL-054 Phase 4 follow-up — persisted run history
                    // for the observability "Automation" tab.
                    (
                        "run_history",
                        nexus_workflow::HANDLER_RUN_HISTORY,
                    ),
                    // BL-054 Phase 4 follow-up — next-fire timestamp
                    // for cron-triggered workflows so the Automation
                    // tab can render an actual schedule preview.
                    (
                        "next_fire",
                        nexus_workflow::core_plugin::HANDLER_NEXT_FIRE,
                    ),
                ]),
            ),
            forge_root,
            Box::new(WorkflowCorePlugin::open_full(
                workflows_dir,
                load_digest_config(forge_root),
                load_webhook_config(forge_root),
            )),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.workflow")?;
    Ok(())
}
