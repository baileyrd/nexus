//! Templates plugin registration.
//!
//! Page-template subsystem. Holds the forge root and serves
//! list/get/render/apply/reload over IPC. Built-ins are included
//! automatically; user templates live at `<forge>/.forge/templates/`.

use std::sync::Arc;

use anyhow::Result;
use nexus_kernel::EventBus;
use nexus_plugins::PluginLoader;
use nexus_templates::TemplatesCorePlugin;

use super::{core_manifest_with_ipc, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.templates",
                "Templates",
                LifecycleFlags::NONE,
                &with_v1_aliases(&[
                    ("list", nexus_templates::HANDLER_LIST),
                    ("get", nexus_templates::HANDLER_GET),
                    ("render", nexus_templates::HANDLER_RENDER),
                    ("apply", nexus_templates::HANDLER_APPLY),
                    ("reload", nexus_templates::HANDLER_RELOAD),
                ]),
            ),
            forge_root,
            Box::new(TemplatesCorePlugin::open(forge_root.to_path_buf())),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.templates")?;
    Ok(())
}
