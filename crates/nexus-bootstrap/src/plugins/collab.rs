//! BL-143 Phase 2.2 — `com.nexus.collab` core plugin registration.
//!
//! Registers [`nexus_collab::core_plugin::CollabCorePlugin`] with one
//! IPC handler: `publish_presence`. The handler stamps the configured
//! peer identity onto a [`nexus_collab::PresenceEvent`] and publishes
//! it on the kernel bus — the existing Phase 1.5 [`ReconnectingClient`]
//! (started from [`crate::collab::start_if_enabled`]) forwards the
//! event to the relay.
//!
//! The plugin is registered unconditionally so its handler is available
//! for the shell to call even before `[collab]` is configured — in
//! that case the handler returns `"collab not configured"` and the
//! frontend treats it as a signal to stop sending. Registering
//! conditionally would force a runtime restart to turn collab on.

use std::sync::Arc;

use anyhow::Result;
use nexus_collab::core_plugin::{
    CollabCorePlugin, LocalPeer, HANDLER_PUBLISH_PRESENCE, HANDLER_RELAY_STATUS,
    HANDLER_START_RELAY, HANDLER_STOP_RELAY, PLUGIN_ID,
};
use nexus_kernel::EventBus;
use nexus_plugins::PluginLoader;

use crate::collab::load_config;

use super::{core_manifest_with_ipc, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    // The same [collab] block that `start_if_enabled` consumes — a
    // single source of truth so identity stamping and the bridge agree
    // on `peer_id` / `display_name`.
    let cfg = load_config(forge_root);
    let identity = if cfg.enabled && !cfg.peer_id.is_empty() && !cfg.display_name.is_empty() {
        Some(LocalPeer {
            user_id: cfg.peer_id.clone(),
            display_name: cfg.display_name.clone(),
        })
    } else {
        None
    };
    let plugin = CollabCorePlugin::new(Some(Arc::clone(event_bus)), identity);
    loader
        .register_core(
            core_manifest_with_ipc(
                PLUGIN_ID,
                "Collaboration",
                LifecycleFlags::NONE,
                &with_v1_aliases(&[
                    ("publish_presence", HANDLER_PUBLISH_PRESENCE),
                    // BL-143 Phase 2.3 — Share this forge: relay-host
                    // IPC surface so the shell can spin up a local
                    // RelayServer without leaving the editor.
                    ("start_relay", HANDLER_START_RELAY),
                    ("stop_relay", HANDLER_STOP_RELAY),
                    ("relay_status", HANDLER_RELAY_STATUS),
                ]),
            ),
            forge_root,
            Box::new(plugin),
        )
        .or_lifecycle_skip(event_bus, PLUGIN_ID)?;
    Ok(())
}
