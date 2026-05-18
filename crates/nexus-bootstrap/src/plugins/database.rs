//! Database plugin registration.
//!
//! `nexus-database` is a pure-logic library (types, validation, formulas,
//! CSV import/export). Its core plugin surfaces only those pure helpers
//! over IPC as `com.nexus.database`; SQL-backed base queries go through
//! `com.nexus.storage` (`base_index` / `base_list` / `base_query`) which
//! is the sole owner of the forge's SQLite database. See
//! docs/architecture/C4.md §4.2.

use std::sync::Arc;

use anyhow::Result;
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
                "com.nexus.database",
                "Database",
                LifecycleFlags::NONE,
                &with_v1_aliases(nexus_database::core_plugin::IPC_HANDLERS),
            ),
            forge_root,
            Box::new(nexus_database::DatabaseCorePlugin::new()),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.database")?;
    Ok(())
}
