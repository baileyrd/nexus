//! Storage plugin registration.
//!
//! Pilot for ADR 0021 (handler versioning): every command is registered
//! under both `<command>` and `<command>.v1` via [`with_v1_aliases`].
//!
//! SD-06 — the `(command-name, handler-id)` pairing lives in
//! [`nexus_storage::core_plugin::IPC_HANDLERS`] next to the handler-id
//! constants; this file just wires the slice into the manifest builder.

use std::sync::Arc;

use anyhow::Result;
use nexus_kernel::EventBus;
use nexus_plugins::PluginLoader;
use nexus_storage::{StorageConfig, StorageCorePlugin};

use super::{core_manifest_with_ipc, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.storage",
                "Storage",
                LifecycleFlags {
                    on_init: true,
                    on_start: true,
                    on_stop: true,
                },
                &with_v1_aliases(nexus_storage::core_plugin::IPC_HANDLERS),
            ),
            forge_root,
            Box::new(StorageCorePlugin::new(
                forge_root.to_path_buf(),
                &StorageConfig::default(),
                Arc::clone(event_bus),
            )),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.storage")?;
    Ok(())
}
