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

use super::{
    core_manifest_with_ipc_and_deps, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt,
};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    let workflows_dir = forge_root.join(".workflows");
    loader
        .register_core(
            core_manifest_with_ipc_and_deps(
                "com.nexus.workflow",
                "Workflow",
                LifecycleFlags::NONE,
                &with_v1_aliases(nexus_workflow::core_plugin::IPC_HANDLERS),
                nexus_workflow::core_plugin::MANIFEST_DEPS,
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
