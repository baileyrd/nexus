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
    options: &crate::BootOptions,
) -> Result<()> {
    // C18 (#370) — long-lived frontends defer the first reconcile to a
    // background pass so boot never blocks on (or times out over) a
    // large forge's initial index build.
    let storage_config = StorageConfig {
        defer_startup_reconcile: options.defer_startup_reconcile,
        ..StorageConfig::default()
    };
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
                &storage_config,
                Arc::clone(event_bus),
            )),
        )
        // Storage is critical: without it, file persistence + indexing
        // are gone, but the rest of the runtime would happily boot and
        // present an editor that silently saves into the void. Use
        // `or_critical` so a lifecycle hang aborts boot instead of
        // degrading silently.
        .or_critical("com.nexus.storage")?;
    Ok(())
}
