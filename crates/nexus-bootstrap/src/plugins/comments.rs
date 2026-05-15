//! Comments plugin registration.
//!
//! BL-050. Side-margin comment threads anchored to stable block ids
//! (ADR 0017). Storage in `<forge>/.forge/comments/<relpath>.json`.
//! Stateless: every dispatch hits disk fresh.

use std::sync::Arc;

use anyhow::Result;
use nexus_comments::core_plugin::CommentsCorePlugin;
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
                "com.nexus.comments",
                "Comments",
                LifecycleFlags::NONE,
                &with_v1_aliases(&[
                    ("list", nexus_comments::core_plugin::HANDLER_LIST),
                    (
                        "create_thread",
                        nexus_comments::core_plugin::HANDLER_CREATE_THREAD,
                    ),
                    (
                        "add_reply",
                        nexus_comments::core_plugin::HANDLER_ADD_REPLY,
                    ),
                    (
                        "set_resolved",
                        nexus_comments::core_plugin::HANDLER_SET_RESOLVED,
                    ),
                    (
                        "delete_thread",
                        nexus_comments::core_plugin::HANDLER_DELETE_THREAD,
                    ),
                    (
                        "delete_comment",
                        nexus_comments::core_plugin::HANDLER_DELETE_COMMENT,
                    ),
                    (
                        "edit_comment",
                        nexus_comments::core_plugin::HANDLER_EDIT_COMMENT,
                    ),
                ]),
            ),
            forge_root,
            Box::new(CommentsCorePlugin::new(forge_root)),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.comments")?;
    Ok(())
}
