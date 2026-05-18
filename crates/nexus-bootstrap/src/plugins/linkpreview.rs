//! Link preview plugin registration.
//!
//! Outbound HTTP fetcher that backs the canvas link-node overlay in
//! the shell. Stateless; the shell owns the cache. Fetches are
//! best-effort and time-bounded (5 s) so a slow host can't hang a
//! canvas render.

use std::sync::Arc;

use anyhow::Result;
use nexus_kernel::EventBus;
use nexus_linkpreview::core_plugin::LinkPreviewCorePlugin;
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
                "com.nexus.linkpreview",
                "Link preview",
                LifecycleFlags::NONE,
                &with_v1_aliases(nexus_linkpreview::core_plugin::IPC_HANDLERS),
            ),
            forge_root,
            Box::new(LinkPreviewCorePlugin::new()),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.linkpreview")?;
    Ok(())
}
