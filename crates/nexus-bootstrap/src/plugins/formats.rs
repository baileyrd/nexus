//! Formats plugin registration.
//!
//! Notion zip-import / format-export. Wraps the pure-library converters
//! in `nexus-formats::notion` behind two IPC handlers so the shell,
//! CLI plugins, and external clients can drive imports/exports through
//! one path.

use std::sync::Arc;

use anyhow::Result;
use nexus_formats::FormatsCorePlugin;
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
                "com.nexus.formats",
                "Formats",
                LifecycleFlags::NONE,
                &with_v1_aliases(&[
                    ("import_notion", nexus_formats::HANDLER_IMPORT_NOTION),
                    ("export_notion", nexus_formats::HANDLER_EXPORT_NOTION),
                ]),
            ),
            forge_root,
            Box::new(FormatsCorePlugin::open(forge_root.to_path_buf())),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.formats")?;
    Ok(())
}
