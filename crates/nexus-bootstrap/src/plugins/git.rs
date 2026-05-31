//! Git plugin registration.
//!
//! Wraps `GitWorker` behind IPC and publishes bus events
//! (branch_changed, commit, dirty_changed) for any plugin or UI that
//! subscribes to `com.nexus.git.*`.

use std::sync::Arc;

use anyhow::Result;
use nexus_git::GitCorePlugin;
use nexus_kernel::EventBus;
use nexus_plugins::PluginLoader;

use super::{
    core_manifest_with_ipc_and_deps, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt,
};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    loader
        .register_core(
            core_manifest_with_ipc_and_deps(
                "com.nexus.git",
                "Git",
                LifecycleFlags {
                    on_init: true,
                    on_start: true,
                    on_stop: true,
                },
                &with_v1_aliases(nexus_git::core_plugin::IPC_HANDLERS),
                nexus_git::core_plugin::MANIFEST_DEPS,
            ),
            forge_root,
            Box::new(GitCorePlugin::new(
                forge_root.to_path_buf(),
                Some(Arc::clone(event_bus)),
            )),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.git")?;
    Ok(())
}
