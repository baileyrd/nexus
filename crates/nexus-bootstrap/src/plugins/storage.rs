//! Storage plugin registration.
//!
//! Pilot for ADR 0021 (handler versioning): every command is registered
//! under both `<command>` and `<command>.v1` via [`with_v1_aliases`].

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
) -> Result<()> {
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
                &with_v1_aliases(&[
                    (
                        "query_files",
                        nexus_storage::core_plugin::HANDLER_QUERY_FILES,
                    ),
                    ("read_file", nexus_storage::core_plugin::HANDLER_READ_FILE),
                    ("backlinks", nexus_storage::core_plugin::HANDLER_BACKLINKS),
                    (
                        "backlinks_to_block",
                        nexus_storage::core_plugin::HANDLER_BACKLINKS_TO_BLOCK,
                    ),
                    (
                        "import_forge",
                        nexus_storage::core_plugin::HANDLER_IMPORT_FORGE,
                    ),
                    (
                        "find_in_files",
                        nexus_storage::core_plugin::HANDLER_FIND_IN_FILES,
                    ),
                    (
                        "replace_in_files",
                        nexus_storage::core_plugin::HANDLER_REPLACE_IN_FILES,
                    ),
                    (
                        "read_frontmatter",
                        nexus_storage::core_plugin::HANDLER_READ_FRONTMATTER,
                    ),
                    (
                        "write_default_gitignore",
                        nexus_storage::core_plugin::HANDLER_WRITE_DEFAULT_GITIGNORE,
                    ),
                    (
                        "query_tasks",
                        nexus_storage::core_plugin::HANDLER_QUERY_TASKS,
                    ),
                    (
                        "graph_stats",
                        nexus_storage::core_plugin::HANDLER_GRAPH_STATS,
                    ),
                    (
                        "rebuild_index",
                        nexus_storage::core_plugin::HANDLER_REBUILD_INDEX,
                    ),
                    ("search", nexus_storage::core_plugin::HANDLER_SEARCH),
                    ("write_file", nexus_storage::core_plugin::HANDLER_WRITE_FILE),
                    (
                        "note_append",
                        nexus_storage::core_plugin::HANDLER_NOTE_APPEND,
                    ),
                    (
                        "write_vault_file",
                        nexus_storage::core_plugin::HANDLER_WRITE_VAULT_FILE,
                    ),
                    (
                        "delete_file",
                        nexus_storage::core_plugin::HANDLER_DELETE_FILE,
                    ),
                    (
                        "file_exists",
                        nexus_storage::core_plugin::HANDLER_FILE_EXISTS,
                    ),
                    (
                        "rebuild_search_index",
                        nexus_storage::core_plugin::HANDLER_REBUILD_SEARCH_INDEX,
                    ),
                    (
                        "toggle_task",
                        nexus_storage::core_plugin::HANDLER_TOGGLE_TASK,
                    ),
                    (
                        "outgoing_links",
                        nexus_storage::core_plugin::HANDLER_OUTGOING_LINKS,
                    ),
                    (
                        "unresolved_links",
                        nexus_storage::core_plugin::HANDLER_UNRESOLVED_LINKS,
                    ),
                    (
                        "graph_neighbors",
                        nexus_storage::core_plugin::HANDLER_GRAPH_NEIGHBORS,
                    ),
                    (
                        "list_all_links",
                        nexus_storage::core_plugin::HANDLER_LIST_ALL_LINKS,
                    ),
                    ("query_tags", nexus_storage::core_plugin::HANDLER_QUERY_TAGS),
                    (
                        "vector_insert",
                        nexus_storage::core_plugin::HANDLER_VECTOR_INSERT,
                    ),
                    (
                        "vector_query",
                        nexus_storage::core_plugin::HANDLER_VECTOR_QUERY,
                    ),
                    (
                        "vector_delete_by_file",
                        nexus_storage::core_plugin::HANDLER_VECTOR_DELETE_BY_FILE,
                    ),
                    (
                        "vectorstore_count",
                        nexus_storage::core_plugin::HANDLER_VECTORSTORE_COUNT,
                    ),
                    (
                        "query_blocks",
                        nexus_storage::core_plugin::HANDLER_QUERY_BLOCKS,
                    ),
                    (
                        "config_read",
                        nexus_storage::core_plugin::HANDLER_CONFIG_READ,
                    ),
                    (
                        "config_reset",
                        nexus_storage::core_plugin::HANDLER_CONFIG_RESET,
                    ),
                    (
                        "settings_read",
                        nexus_storage::core_plugin::HANDLER_SETTINGS_READ,
                    ),
                    (
                        "settings_write",
                        nexus_storage::core_plugin::HANDLER_SETTINGS_WRITE,
                    ),
                    (
                        "query_symbol",
                        nexus_storage::core_plugin::HANDLER_QUERY_SYMBOL,
                    ),
                    (
                        "entity_search",
                        nexus_storage::core_plugin::HANDLER_ENTITY_SEARCH,
                    ),
                    (
                        "entity_get",
                        nexus_storage::core_plugin::HANDLER_ENTITY_GET,
                    ),
                    (
                        "entity_relations",
                        nexus_storage::core_plugin::HANDLER_ENTITY_RELATIONS,
                    ),
                    (
                        "entity_upsert",
                        nexus_storage::core_plugin::HANDLER_ENTITY_UPSERT,
                    ),
                    (
                        "entity_find_duplicates",
                        nexus_storage::core_plugin::HANDLER_ENTITY_FIND_DUPLICATES,
                    ),
                    (
                        "entity_decay_relations",
                        nexus_storage::core_plugin::HANDLER_ENTITY_DECAY_RELATIONS,
                    ),
                    (
                        "entity_merge",
                        nexus_storage::core_plugin::HANDLER_ENTITY_MERGE,
                    ),
                    ("base_index", nexus_storage::core_plugin::HANDLER_BASE_INDEX),
                    ("base_list", nexus_storage::core_plugin::HANDLER_BASE_LIST),
                    ("base_query", nexus_storage::core_plugin::HANDLER_BASE_QUERY),
                    ("base_load", nexus_storage::core_plugin::HANDLER_BASE_LOAD),
                    ("list_dir", nexus_storage::core_plugin::HANDLER_LIST_DIR),
                    (
                        "create_file",
                        nexus_storage::core_plugin::HANDLER_CREATE_FILE,
                    ),
                    ("create_dir", nexus_storage::core_plugin::HANDLER_CREATE_DIR),
                    (
                        "rename_entry",
                        nexus_storage::core_plugin::HANDLER_RENAME_ENTRY,
                    ),
                    (
                        "delete_entry",
                        nexus_storage::core_plugin::HANDLER_DELETE_ENTRY,
                    ),
                    (
                        "canvas_read",
                        nexus_storage::core_plugin::HANDLER_CANVAS_READ,
                    ),
                    (
                        "canvas_write",
                        nexus_storage::core_plugin::HANDLER_CANVAS_WRITE,
                    ),
                    (
                        "canvas_patch",
                        nexus_storage::core_plugin::HANDLER_CANVAS_PATCH,
                    ),
                    (
                        "canvas_nodes",
                        nexus_storage::core_plugin::HANDLER_CANVAS_NODES,
                    ),
                    (
                        "canvas_edges",
                        nexus_storage::core_plugin::HANDLER_CANVAS_EDGES,
                    ),
                    (
                        "base_record_create",
                        nexus_storage::core_plugin::HANDLER_BASE_RECORD_CREATE,
                    ),
                    (
                        "base_record_update",
                        nexus_storage::core_plugin::HANDLER_BASE_RECORD_UPDATE,
                    ),
                    (
                        "base_record_delete",
                        nexus_storage::core_plugin::HANDLER_BASE_RECORD_DELETE,
                    ),
                    (
                        "base_property_create",
                        nexus_storage::core_plugin::HANDLER_BASE_PROPERTY_CREATE,
                    ),
                    (
                        "base_property_update",
                        nexus_storage::core_plugin::HANDLER_BASE_PROPERTY_UPDATE,
                    ),
                    (
                        "base_property_delete",
                        nexus_storage::core_plugin::HANDLER_BASE_PROPERTY_DELETE,
                    ),
                    (
                        "base_view_create",
                        nexus_storage::core_plugin::HANDLER_BASE_VIEW_CREATE,
                    ),
                    (
                        "base_view_update",
                        nexus_storage::core_plugin::HANDLER_BASE_VIEW_UPDATE,
                    ),
                    (
                        "base_view_delete",
                        nexus_storage::core_plugin::HANDLER_BASE_VIEW_DELETE,
                    ),
                    (
                        "base_create",
                        nexus_storage::core_plugin::HANDLER_BASE_CREATE,
                    ),
                    (
                        "base_property_rename",
                        nexus_storage::core_plugin::HANDLER_BASE_PROPERTY_RENAME,
                    ),
                    (
                        "base_record_soft_delete",
                        nexus_storage::core_plugin::HANDLER_BASE_RECORD_SOFT_DELETE,
                    ),
                    (
                        "base_record_restore",
                        nexus_storage::core_plugin::HANDLER_BASE_RECORD_RESTORE,
                    ),
                    (
                        "obsidian_base_query",
                        nexus_storage::core_plugin::HANDLER_OBSIDIAN_BASE_QUERY,
                    ),
                ]),
            ),
            forge_root,
            Box::new(StorageCorePlugin::new(
                forge_root.to_path_buf(),
                &StorageConfig::default(),
                Arc::clone(event_bus),
            )),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.storage")?;
    Ok(())
}
