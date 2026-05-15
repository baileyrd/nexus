//! Security plugin registration.
//!
//! Registered first so audit events are available before other plugins emit.

use std::sync::Arc;

use anyhow::Result;
use nexus_kernel::EventBus;
use nexus_plugins::PluginLoader;
use nexus_security::SecurityCorePlugin;

use super::{core_manifest_with_ipc, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.security",
                "Security",
                LifecycleFlags {
                    on_init: true,
                    on_start: true,
                    on_stop: true,
                },
                &with_v1_aliases(&[
                    ("get_secret", nexus_security::core_plugin::HANDLER_GET_SECRET),
                    ("set_secret", nexus_security::core_plugin::HANDLER_SET_SECRET),
                    ("delete_secret", nexus_security::core_plugin::HANDLER_DELETE_SECRET),
                    ("list_secret_names", nexus_security::core_plugin::HANDLER_LIST_SECRET_NAMES),
                    ("query_audit_log", nexus_security::core_plugin::HANDLER_QUERY_AUDIT_LOG),
                    ("clear_audit_log", nexus_security::core_plugin::HANDLER_CLEAR_AUDIT_LOG),
                    ("metrics_snapshot", nexus_security::core_plugin::HANDLER_METRICS_SNAPSHOT),
                ]),
            ),
            forge_root,
            Box::new(SecurityCorePlugin::new(Some(Arc::clone(event_bus)))),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.security")?;
    Ok(())
}
