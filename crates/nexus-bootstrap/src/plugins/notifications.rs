//! Notifications plugin registration.
//!
//! BL-133 / BL-135 — multi-channel notification dispatcher with a
//! forge-local routing config. Source-tagged sends consult
//! `<forge>/.forge/notifications.toml` to pick channels; explicit-
//! channel sends bypass the router. When `notifications.toml` is
//! absent the bootstrap falls back to the legacy
//! `config.toml::[notifications.*]` blocks so a forge that
//! predates BL-135 keeps working without editing.

use std::sync::Arc;

use anyhow::Result;
use nexus_kernel::EventBus;
use nexus_notifications::core_plugin::NotificationsCorePlugin;
use nexus_plugins::PluginLoader;

use crate::load_notifications_config;

use super::{core_manifest_with_ipc, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    let (notifications_config, notifications_config_path) =
        load_notifications_config(forge_root);
    let inbox_db_path = forge_root.join(nexus_notifications::INBOX_DB_RELPATH);
    let notifications_plugin = nexus_notifications::core_plugin::NotificationsCorePlugin::from_config_with_inbox(
        Some(Arc::clone(event_bus)),
        notifications_config,
        notifications_config_path,
        Some(inbox_db_path),
    )
    .unwrap_or_else(|err| {
        tracing::warn!(
            %err,
            "notifications.toml: resolve_sources failed; falling back to empty router"
        );
        NotificationsCorePlugin::with_defaults(
            Some(Arc::clone(event_bus)),
            String::new(),
            String::new(),
            String::new(),
            nexus_notifications::SmtpConfig::default(),
        )
    });
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.notifications",
                "Notifications",
                LifecycleFlags {
                    on_init: false,
                    on_start: true,
                    on_stop: false,
                },
                &with_v1_aliases(&[
                    ("send", nexus_notifications::core_plugin::HANDLER_SEND),
                    (
                        "inbox_list",
                        nexus_notifications::core_plugin::HANDLER_INBOX_LIST,
                    ),
                    (
                        "inbox_mark_read",
                        nexus_notifications::core_plugin::HANDLER_INBOX_MARK_READ,
                    ),
                    (
                        "inbox_dismiss",
                        nexus_notifications::core_plugin::HANDLER_INBOX_DISMISS,
                    ),
                    (
                        "inbox_stats",
                        nexus_notifications::core_plugin::HANDLER_INBOX_STATS,
                    ),
                ]),
            ),
            forge_root,
            Box::new(notifications_plugin),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.notifications")?;
    Ok(())
}
