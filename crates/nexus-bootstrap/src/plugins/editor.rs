//! Editor plugin registration.
//!
//! BL-074 editor wiring: each `apply_transaction` routes through the
//! CRDT publisher, which mirrors the session in a `CrdtDoc`, publishes
//! per-op envelopes on `com.nexus.editor.ops.<relpath>`, and persists
//! to `.forge/.editor/crdt/<sha>.json` on close.

use std::sync::Arc;

use anyhow::Result;
use nexus_editor::EditorCorePlugin;
use nexus_kernel::EventBus;
use nexus_plugins::PluginLoader;

use crate::crdt_publisher;

use super::{core_manifest_with_ipc, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.editor",
                "Editor",
                LifecycleFlags {
                    on_init: true,
                    ..LifecycleFlags::NONE
                },
                &with_v1_aliases(&[
                    ("open", nexus_editor::core_plugin::HANDLER_OPEN),
                    ("close", nexus_editor::core_plugin::HANDLER_CLOSE),
                    ("get_tree", nexus_editor::core_plugin::HANDLER_GET_TREE),
                    ("save", nexus_editor::core_plugin::HANDLER_SAVE),
                    (
                        "apply_transaction",
                        nexus_editor::core_plugin::HANDLER_APPLY_TRANSACTION,
                    ),
                    ("undo", nexus_editor::core_plugin::HANDLER_UNDO),
                    ("redo", nexus_editor::core_plugin::HANDLER_REDO),
                    ("list_open", nexus_editor::core_plugin::HANDLER_LIST_OPEN),
                    (
                        "sync_content",
                        nexus_editor::core_plugin::HANDLER_SYNC_CONTENT,
                    ),
                    (
                        "get_markdown",
                        nexus_editor::core_plugin::HANDLER_GET_MARKDOWN,
                    ),
                    (
                        "stamp_block",
                        nexus_editor::core_plugin::HANDLER_STAMP_BLOCK,
                    ),
                    (
                        "execute_database_view",
                        nexus_editor::core_plugin::HANDLER_EXECUTE_DATABASE_VIEW,
                    ),
                    (
                        "resolve_block_link",
                        nexus_editor::core_plugin::HANDLER_RESOLVE_BLOCK_LINK,
                    ),
                ]),
            ),
            forge_root,
            {
                let mut plugin = EditorCorePlugin::with_event_bus(
                    forge_root.to_path_buf(),
                    Arc::clone(event_bus),
                );
                // BL-074 editor wiring: each apply_transaction routes
                // through the publisher, which mirrors the session in
                // a CrdtDoc, publishes per-op envelopes on
                // `com.nexus.editor.ops.<relpath>`, and persists to
                // `.forge/.editor/crdt/<sha>.json` on close.
                let publisher = Arc::new(crdt_publisher::CrdtPublisher::new(
                    forge_root.to_path_buf(),
                    Arc::clone(event_bus),
                ));
                // BL-007 pull-landing wiring: when `nexus-git`'s state
                // poller emits `com.nexus.git.commit` (HEAD advanced
                // — including the merge / fast-forward end of a
                // `git pull`), the subscriber re-reads each open
                // session's `.forge/.editor/crdt/<sha>.json` and
                // absorbs any ops the merge driver unioned in. The
                // thread holds a `Weak` to the publisher's inner
                // state, so when the editor plugin's `on_stop`
                // releases the last `Arc` the thread exits on its
                // next tick — no explicit shutdown signal needed.
                let _pull_landing_handle = publisher.start_pull_landing_subscriber();
                plugin.set_op_observer(publisher);
                Box::new(plugin)
            },
        )
        .or_lifecycle_skip(event_bus, "com.nexus.editor")?;
    Ok(())
}
