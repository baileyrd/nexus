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

use super::{core_manifest_with_ipc, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.git",
                "Git",
                LifecycleFlags {
                    on_init: true,
                    on_start: true,
                    on_stop: true,
                },
                &with_v1_aliases(&[
                    ("status", nexus_git::core_plugin::HANDLER_STATUS),
                    ("log", nexus_git::core_plugin::HANDLER_LOG),
                    ("branches", nexus_git::core_plugin::HANDLER_BRANCHES),
                    ("file_status", nexus_git::core_plugin::HANDLER_FILE_STATUS),
                    ("diff_file", nexus_git::core_plugin::HANDLER_DIFF_FILE),
                    ("stage_file", nexus_git::core_plugin::HANDLER_STAGE_FILE),
                    ("unstage_file", nexus_git::core_plugin::HANDLER_UNSTAGE_FILE),
                    ("commit", nexus_git::core_plugin::HANDLER_COMMIT),
                    ("stage_all", nexus_git::core_plugin::HANDLER_STAGE_ALL),
                    ("unstage_all", nexus_git::core_plugin::HANDLER_UNSTAGE_ALL),
                    ("file_statuses", nexus_git::core_plugin::HANDLER_FILE_STATUSES),
                    ("diff_staged", nexus_git::core_plugin::HANDLER_DIFF_STAGED),
                    ("switch_branch", nexus_git::core_plugin::HANDLER_SWITCH_BRANCH),
                    ("create_branch", nexus_git::core_plugin::HANDLER_CREATE_BRANCH),
                    ("delete_branch", nexus_git::core_plugin::HANDLER_DELETE_BRANCH),
                    ("push", nexus_git::core_plugin::HANDLER_PUSH),
                    ("stage_hunks", nexus_git::core_plugin::HANDLER_STAGE_HUNKS),
                    ("unstage_hunks", nexus_git::core_plugin::HANDLER_UNSTAGE_HUNKS),
                    ("stash_push", nexus_git::core_plugin::HANDLER_STASH_PUSH),
                    ("stash_list", nexus_git::core_plugin::HANDLER_STASH_LIST),
                    ("stash_pop", nexus_git::core_plugin::HANDLER_STASH_POP),
                    ("stash_drop", nexus_git::core_plugin::HANDLER_STASH_DROP),
                    ("list_tags", nexus_git::core_plugin::HANDLER_LIST_TAGS),
                    ("create_tag", nexus_git::core_plugin::HANDLER_CREATE_TAG),
                    ("delete_tag", nexus_git::core_plugin::HANDLER_DELETE_TAG),
                    ("push_tags", nexus_git::core_plugin::HANDLER_PUSH_TAGS),
                    ("lfs_status", nexus_git::core_plugin::HANDLER_LFS_STATUS),
                    ("rebase", nexus_git::core_plugin::HANDLER_REBASE),
                    ("abort_rebase", nexus_git::core_plugin::HANDLER_ABORT_REBASE),
                    ("cherry_pick", nexus_git::core_plugin::HANDLER_CHERRY_PICK),
                    ("abort_cherry_pick", nexus_git::core_plugin::HANDLER_ABORT_CHERRY_PICK),
                    ("conflict_files", nexus_git::core_plugin::HANDLER_CONFLICT_FILES),
                    ("abort_merge", nexus_git::core_plugin::HANDLER_ABORT_MERGE),
                    ("conflict_versions", nexus_git::core_plugin::HANDLER_CONFLICT_VERSIONS),
                    ("merge", nexus_git::core_plugin::HANDLER_MERGE),
                    ("blame", nexus_git::core_plugin::HANDLER_BLAME),
                    ("discard_hunks", nexus_git::core_plugin::HANDLER_DISCARD_HUNKS),
                    ("file_log", nexus_git::core_plugin::HANDLER_FILE_LOG),
                ]),
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
