//! Nexus runtime bootstrap for CLI/TUI invokers.
//!
//! This crate is the one place in the codebase that knows how to assemble a
//! complete Nexus runtime: kernel + plugin loader + every in-tree core plugin
//! + an invoker plugin representing the CLI or TUI.
//!
//! Call [`build_cli_runtime`] or [`build_tui_runtime`] with the forge root.
//! You get back a [`Runtime`] holding a [`KernelPluginContext`] — every
//! subsystem call the invoker makes should route through
//! `runtime.context.ipc_call(...)` instead of importing subsystem crates
//! directly.
//!
//! Community plugins discovered under `.forge/plugins/` are NOT loaded here;
//! that's a job for the plugin manager spun up lazily by the invoker.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use nexus_kernel::{
    Capability, CapabilitySet, EventBus, IpcDispatcher, Kernel, KernelConfig, KernelPluginContext,
    PluginError as KernelPluginError,
};
use nexus_plugins::{
    parse_manifest, CorePlugin, PluginError, PluginLoader, PluginManifest, SharedPluginLoader,
};

pub mod agent;
pub mod database;
pub mod storage;
pub mod terminal;

/// Render a markdown note to a standalone HTML string.
///
/// Re-exported from `nexus-formats` (pure format library, no SQLite).
pub use nexus_formats::export_to_html;

/// Canvas data types and pure (de)serialization helpers, re-exported from
/// `nexus-formats` so CLI/TUI canvas commands can parse and mutate canvas
/// files without pulling in the SQLite-backed `nexus-storage` crate.
pub use nexus_formats::{CanvasEdge, CanvasEdgeType, CanvasFile, CanvasNode, CanvasNodeType};
/// Parse a `.canvas` JSON string.
pub use nexus_formats::canvas::parse as parse_canvas;
/// Serialize a [`CanvasFile`] to pretty-printed JSON.
pub use nexus_formats::canvas::serialize as serialize_canvas;

/// Plugin id for the in-tree Nexus CLI invoker.
pub const CLI_PLUGIN_ID: &str = "com.nexus.cli";
/// Plugin id for the in-tree Nexus TUI invoker.
pub const TUI_PLUGIN_ID: &str = "com.nexus.tui";

/// Assembled Nexus runtime handed back to the invoker.
///
/// The invoker (CLI or TUI) is itself registered as a Core plugin holding
/// `Capability::ALL`, so calls made through [`Runtime::context`] go through
/// the same kernel surface as any other plugin.
pub struct Runtime {
    /// Kernel owning the event bus, KV store, and plugin registry.
    pub kernel: Kernel,
    /// Plugin-facing kernel context for the invoker. Use for `ipc_call`,
    /// `publish`, `subscribe`, `kv_*`, etc.
    pub context: KernelPluginContext,
    /// Shared plugin loader. Must be kept alive for the lifetime of the
    /// runtime — the context holds an `Arc<dyn IpcDispatcher>` pointing here.
    pub loader: Arc<SharedPluginLoader>,
}

/// Build a runtime with the CLI registered as the invoker plugin.
///
/// # Errors
/// Returns an error if the forge cannot be opened, the kernel config cannot
/// be loaded, or any core plugin fails to register or initialize.
pub fn build_cli_runtime(forge_root: PathBuf) -> Result<Runtime> {
    build(forge_root, CLI_PLUGIN_ID, "Nexus CLI")
}

/// Create an empty forge at `forge_root`.
///
/// This is the one storage operation the CLI cannot do through `ipc_call`:
/// the storage plugin's `on_init` opens an existing forge, so the forge must
/// exist before a runtime can be built. Exposed from the bootstrap crate so
/// `nexus-cli` does not need to depend on `nexus-storage` directly just for
/// `nexus forge init`.
///
/// # Errors
/// Propagates any [`nexus_storage::StorageError`] from `StorageEngine::init`.
pub fn init_forge(forge_root: &std::path::Path) -> Result<()> {
    nexus_storage::StorageEngine::init(forge_root)
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("failed to initialise forge: {e}"))
}

/// Build a runtime with the TUI registered as the invoker plugin.
///
/// # Errors
/// See [`build_cli_runtime`].
pub fn build_tui_runtime(forge_root: PathBuf) -> Result<Runtime> {
    build(forge_root, TUI_PLUGIN_ID, "Nexus TUI")
}

fn build(forge_root: PathBuf, invoker_id: &'static str, invoker_name: &str) -> Result<Runtime> {
    let config = KernelConfig::load(&forge_root)
        .with_context(|| format!("failed to load kernel config at '{}'", forge_root.display()))?;

    // The KV store lives under `.forge/kv.sqlite3`. Create the dir here —
    // kernel is backend-agnostic now, so dir creation is bootstrap's job.
    let forge_dir = forge_root.join(".forge");
    std::fs::create_dir_all(&forge_dir)
        .with_context(|| format!("failed to create forge dir '{}'", forge_dir.display()))?;
    let kv_path = forge_dir.join("kv.sqlite3");
    let kv_store: Arc<dyn nexus_kernel::KvStore> =
        Arc::new(nexus_kv::SqliteKvStore::open(&kv_path).with_context(|| {
            format!("failed to open kernel KV store at '{}'", kv_path.display())
        })?);

    let kernel = Kernel::new(config, kv_store).context("failed to build kernel")?;
    let event_bus: Arc<EventBus> = kernel.event_bus();
    let kv_store = kernel.kv_store();

    let plugins_dir = forge_root.join(".forge").join("plugins");
    let mut loader = PluginLoader::new(&plugins_dir);
    for extra in &kernel.config().plugin_search_paths {
        loader.add_search_path(extra.clone());
    }
    loader.set_event_bus(Arc::clone(&event_bus));

    // Register every in-tree core plugin. Order matters where lifecycle hooks
    // of later plugins rely on earlier ones publishing events; in practice
    // each plugin is independent today.
    register_core_plugins(&mut loader, &forge_root, &event_bus)?;

    // Register the invoker (CLI or TUI) as a Core plugin so it holds a real
    // plugin identity with Capability::ALL.
    let invoker_manifest = invoker_manifest(invoker_id, invoker_name);
    loader
        .register_core(
            invoker_manifest,
            &forge_root,
            Box::new(InvokerPlugin {
                id: invoker_id.to_string(),
            }),
        )
        .map_err(|e| anyhow::anyhow!("failed to register invoker plugin {invoker_id}: {e}"))?;

    let shared = Arc::new(SharedPluginLoader::new(loader));
    let dispatcher: Arc<dyn IpcDispatcher> = Arc::clone(&shared) as Arc<dyn IpcDispatcher>;

    // Hand the AI plugin its own KernelPluginContext so `ask`/`index_file`
    // handlers can issue nested `ipc_call`s into storage through the same
    // plugin-facing surface the invoker uses — no raw-dispatcher leak.
    let ai_ctx = KernelPluginContext::new(
        "com.nexus.ai",
        env!("CARGO_PKG_VERSION"),
        CapabilitySet::from_iter(Capability::ALL.iter().copied()),
        Arc::clone(&kv_store),
        Arc::clone(&event_bus),
        &forge_root,
        Some(Arc::clone(&dispatcher)),
    )
    .context("failed to build kernel plugin context for com.nexus.ai")?;
    shared
        .wire_context("com.nexus.ai", Arc::new(ai_ctx))
        .map_err(|e| anyhow::anyhow!("failed to wire AI plugin context: {e}"))?;

    // Agent plugin needs its own context for two reasons: driving
    // `com.nexus.ai::stream_chat` for planning, and dispatching every
    // `ToolCall` the resulting plan emits to whatever target plugin
    // the agent picked.
    let agent_ctx = KernelPluginContext::new(
        "com.nexus.agent",
        env!("CARGO_PKG_VERSION"),
        CapabilitySet::from_iter(Capability::ALL.iter().copied()),
        Arc::clone(&kv_store),
        Arc::clone(&event_bus),
        &forge_root,
        Some(Arc::clone(&dispatcher)),
    )
    .context("failed to build kernel plugin context for com.nexus.agent")?;
    shared
        .wire_context("com.nexus.agent", Arc::new(agent_ctx))
        .map_err(|e| anyhow::anyhow!("failed to wire agent plugin context: {e}"))?;

    // Same treatment for the editor plugin: its `open`/`save` handlers
    // route through `com.nexus.storage` so reads/writes honour the
    // storage plugin's capability checks and atomic-write semantics.
    let editor_ctx = KernelPluginContext::new(
        "com.nexus.editor",
        env!("CARGO_PKG_VERSION"),
        CapabilitySet::from_iter(Capability::ALL.iter().copied()),
        Arc::clone(&kv_store),
        Arc::clone(&event_bus),
        &forge_root,
        Some(Arc::clone(&dispatcher)),
    )
    .context("failed to build kernel plugin context for com.nexus.editor")?;
    shared
        .wire_context("com.nexus.editor", Arc::new(editor_ctx))
        .map_err(|e| anyhow::anyhow!("failed to wire editor plugin context: {e}"))?;

    // Workflow plugin needs the kernel context so its `run` handler
    // can drive arbitrary plugins via ipc_call.
    let workflow_ctx = KernelPluginContext::new(
        "com.nexus.workflow",
        env!("CARGO_PKG_VERSION"),
        CapabilitySet::from_iter(Capability::ALL.iter().copied()),
        Arc::clone(&kv_store),
        Arc::clone(&event_bus),
        &forge_root,
        Some(Arc::clone(&dispatcher)),
    )
    .context("failed to build kernel plugin context for com.nexus.workflow")?;
    shared
        .wire_context("com.nexus.workflow", Arc::new(workflow_ctx))
        .map_err(|e| anyhow::anyhow!("failed to wire workflow plugin context: {e}"))?;

    let context = KernelPluginContext::new(
        invoker_id,
        env!("CARGO_PKG_VERSION"),
        CapabilitySet::from_iter(Capability::ALL.iter().copied()),
        kv_store,
        event_bus,
        &forge_root,
        Some(dispatcher),
    )
    .context("failed to build kernel plugin context for invoker")?;

    Ok(Runtime {
        kernel,
        context,
        loader: shared,
    })
}

fn register_core_plugins(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    use nexus_agent::AgentCorePlugin;
    use nexus_ai::AiCorePlugin;
    use nexus_skills::SkillsCorePlugin;
    use nexus_workflow::WorkflowCorePlugin;
    use nexus_editor::EditorCorePlugin;
    use nexus_git::GitCorePlugin;
    use nexus_mcp::McpHostPlugin;
    use nexus_security::SecurityCorePlugin;
    use nexus_storage::{StorageConfig, StorageCorePlugin};
    use nexus_terminal::TerminalCorePlugin;
    use nexus_theme::ThemeCorePlugin;

    // Security first so audit events are available before other plugins emit.
    loader
        .register_core(
            core_manifest(
                "com.nexus.security",
                "Security",
                LifecycleFlags {
                    on_init: true,
                    on_start: true,
                    on_stop: true,
                },
            ),
            forge_root,
            Box::new(SecurityCorePlugin::new(Some(Arc::clone(event_bus)))),
        )
        .context("failed to register com.nexus.security")?;

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
                &[
                    (
                        "query_files",
                        nexus_storage::core_plugin::HANDLER_QUERY_FILES,
                    ),
                    ("read_file", nexus_storage::core_plugin::HANDLER_READ_FILE),
                    ("backlinks", nexus_storage::core_plugin::HANDLER_BACKLINKS),
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
                ],
            ),
            forge_root,
            Box::new(StorageCorePlugin::new(
                forge_root.to_path_buf(),
                &StorageConfig::default(),
                Arc::clone(event_bus),
            )),
        )
        .context("failed to register com.nexus.storage")?;

    // `nexus-database` is a pure-logic library (types, validation, formulas,
    // CSV import/export). Its core plugin surfaces only those pure helpers
    // over IPC as `com.nexus.database`; SQL-backed base queries go through
    // `com.nexus.storage` (`base_index` / `base_list` / `base_query`) which
    // is the sole owner of the forge's SQLite database. See
    // ARCHITECTURE.md §4.2.
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.database",
                "Database",
                LifecycleFlags::NONE,
                &[
                    (
                        "csv_import",
                        nexus_database::core_plugin::HANDLER_CSV_IMPORT,
                    ),
                    (
                        "csv_export",
                        nexus_database::core_plugin::HANDLER_CSV_EXPORT,
                    ),
                    (
                        "formula_eval",
                        nexus_database::core_plugin::HANDLER_FORMULA_EVAL,
                    ),
                    (
                        "apply_view",
                        nexus_database::core_plugin::HANDLER_APPLY_VIEW,
                    ),
                ],
            ),
            forge_root,
            Box::new(nexus_database::DatabaseCorePlugin::new()),
        )
        .context("failed to register com.nexus.database")?;

    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.editor",
                "Editor",
                LifecycleFlags {
                    on_init: true,
                    ..LifecycleFlags::NONE
                },
                &[
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
                ],
            ),
            forge_root,
            Box::new(EditorCorePlugin::new(forge_root.to_path_buf())),
        )
        .context("failed to register com.nexus.editor")?;

    // Theme engine — registered as a core plugin so (a) other plugins can
    // call `ipc_call("com.nexus.theme", …)` and subscribe to
    // `com.nexus.theme.changed` events, and (b) the Tauri shell's theme
    // commands are thin adapters over kernel IPC rather than owning
    // engine state directly. See PRD-07.
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.theme",
                "Theme",
                LifecycleFlags::NONE,
                &[
                    (
                        "get_available_themes",
                        nexus_theme::core_plugin::HANDLER_GET_AVAILABLE_THEMES,
                    ),
                    (
                        "apply_theme",
                        nexus_theme::core_plugin::HANDLER_APPLY_THEME,
                    ),
                    (
                        "compute_variables",
                        nexus_theme::core_plugin::HANDLER_COMPUTE_VARIABLES,
                    ),
                    (
                        "get_available_snippets",
                        nexus_theme::core_plugin::HANDLER_GET_AVAILABLE_SNIPPETS,
                    ),
                    (
                        "toggle_snippet",
                        nexus_theme::core_plugin::HANDLER_TOGGLE_SNIPPET,
                    ),
                    (
                        "reorder_snippets",
                        nexus_theme::core_plugin::HANDLER_REORDER_SNIPPETS,
                    ),
                    (
                        "get_theme_config",
                        nexus_theme::core_plugin::HANDLER_GET_THEME_CONFIG,
                    ),
                    ("set_mode", nexus_theme::core_plugin::HANDLER_SET_MODE),
                    (
                        "apply_config",
                        nexus_theme::core_plugin::HANDLER_APPLY_CONFIG,
                    ),
                    (
                        "set_plugin_overrides",
                        nexus_theme::core_plugin::HANDLER_SET_PLUGIN_OVERRIDES,
                    ),
                    ("reload", nexus_theme::core_plugin::HANDLER_RELOAD),
                ],
            ),
            forge_root,
            Box::new(ThemeCorePlugin::with_builtins(Some(Arc::clone(event_bus)))),
        )
        .context("failed to register com.nexus.theme")?;

    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.ai",
                "AI",
                LifecycleFlags {
                    on_init: true,
                    ..LifecycleFlags::NONE
                },
                &[
                    ("ask", nexus_ai::core_plugin::HANDLER_ASK),
                    ("index_file", nexus_ai::core_plugin::HANDLER_INDEX_FILE),
                    (
                        "vectorstore_count",
                        nexus_ai::core_plugin::HANDLER_VECTORSTORE_COUNT,
                    ),
                    ("status", nexus_ai::core_plugin::HANDLER_STATUS),
                    ("config", nexus_ai::core_plugin::HANDLER_CONFIG),
                    (
                        "stream_chat",
                        nexus_ai::core_plugin::HANDLER_STREAM_CHAT,
                    ),
                    (
                        "stream_ask",
                        nexus_ai::core_plugin::HANDLER_STREAM_ASK,
                    ),
                    (
                        "session_load",
                        nexus_ai::core_plugin::HANDLER_SESSION_LOAD,
                    ),
                    (
                        "session_save",
                        nexus_ai::core_plugin::HANDLER_SESSION_SAVE,
                    ),
                    (
                        "session_list",
                        nexus_ai::core_plugin::HANDLER_SESSION_LIST,
                    ),
                    (
                        "session_delete",
                        nexus_ai::core_plugin::HANDLER_SESSION_DELETE,
                    ),
                ],
            ),
            forge_root,
            Box::new(AiCorePlugin::new()),
        )
        .context("failed to register com.nexus.ai")?;

    // Skills — PRD-13 scaffold. Read-mostly surface over
    // `.forge/skills/`. Agents + UI consult it over IPC so no
    // consumer links `nexus-skills` directly.
    let skills_dir = forge_root.join(".forge").join("skills");
    match nexus_skills::seed_builtins(&skills_dir) {
        Ok(report) if !report.created.is_empty() => tracing::info!(
            path = %skills_dir.display(),
            created = ?report.created,
            skipped = report.skipped.len(),
            "seeded built-in skills"
        ),
        Ok(_) => {}
        Err(err) => tracing::warn!(
            path = %skills_dir.display(),
            %err,
            "failed to seed built-in skills — continuing with whatever is already on disk"
        ),
    }
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.skills",
                "Skills",
                LifecycleFlags::NONE,
                &[
                    ("list", nexus_skills::HANDLER_LIST),
                    ("get", nexus_skills::HANDLER_GET),
                    (
                        "list_by_context",
                        nexus_skills::HANDLER_LIST_BY_CONTEXT,
                    ),
                    (
                        "triggered_by",
                        nexus_skills::HANDLER_TRIGGERED_BY,
                    ),
                    ("reload", nexus_skills::HANDLER_RELOAD),
                    ("render", nexus_skills::HANDLER_RENDER),
                ],
            ),
            forge_root,
            Box::new(SkillsCorePlugin::open(skills_dir)),
        )
        .context("failed to register com.nexus.skills")?;

    // Workflow — PRD-16 scaffold. Read-mostly surface over
    // `.workflows/` TOML files. Library stays kernel-free; this
    // plugin is the only integration point.
    let workflows_dir = forge_root.join(".workflows");
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.workflow",
                "Workflow",
                LifecycleFlags::NONE,
                &[
                    ("list", nexus_workflow::HANDLER_LIST),
                    ("get", nexus_workflow::HANDLER_GET),
                    ("reload", nexus_workflow::HANDLER_RELOAD),
                    ("validate", nexus_workflow::HANDLER_VALIDATE),
                    ("run", nexus_workflow::HANDLER_RUN),
                ],
            ),
            forge_root,
            Box::new(WorkflowCorePlugin::open(workflows_dir)),
        )
        .context("failed to register com.nexus.workflow")?;

    // Agent system — PRD-15 scaffold. Thin dispatch surface over
    // `nexus-agent::{LlmAgent, PlanExecutor}`; bridges to `com.nexus.ai`
    // for planning and to arbitrary plugins for tool calls via the
    // `KernelPluginContext` wired below.
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.agent",
                "Agent",
                LifecycleFlags::NONE,
                &[
                    ("plan", nexus_agent::HANDLER_PLAN),
                    ("run", nexus_agent::HANDLER_RUN),
                    ("run_plan", nexus_agent::HANDLER_RUN_PLAN),
                    ("execute_step", nexus_agent::HANDLER_EXECUTE_STEP),
                    ("history_list", nexus_agent::HANDLER_HISTORY_LIST),
                    ("history_get", nexus_agent::HANDLER_HISTORY_GET),
                    ("history_delete", nexus_agent::HANDLER_HISTORY_DELETE),
                ],
            ),
            forge_root,
            Box::new(AgentCorePlugin::new()),
        )
        .context("failed to register com.nexus.agent")?;

    // MCP Host orchestrator — loads mcp.toml, lazily connects to external MCP
    // servers, exposes list_tools / call_tool / list_resources / list_prompts
    // over IPC so any plugin or invoker can reach external tools without
    // linking the rmcp crate directly.
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.mcp.host",
                "MCP Host",
                LifecycleFlags {
                    on_init: true,
                    on_start: true,
                    on_stop: true,
                },
                &[
                    (
                        "list_servers",
                        nexus_mcp::core_plugin::HANDLER_LIST_SERVERS,
                    ),
                    ("list_tools", nexus_mcp::core_plugin::HANDLER_LIST_TOOLS),
                    ("call_tool", nexus_mcp::core_plugin::HANDLER_CALL_TOOL),
                    (
                        "list_resources",
                        nexus_mcp::core_plugin::HANDLER_LIST_RESOURCES,
                    ),
                    (
                        "list_prompts",
                        nexus_mcp::core_plugin::HANDLER_LIST_PROMPTS,
                    ),
                    ("connect", nexus_mcp::core_plugin::HANDLER_CONNECT),
                    ("disconnect", nexus_mcp::core_plugin::HANDLER_DISCONNECT),
                ],
            ),
            forge_root,
            Box::new(McpHostPlugin::new(
                forge_root.to_path_buf(),
                Some(Arc::clone(event_bus)),
            )),
        )
        .context("failed to register com.nexus.mcp.host")?;

    // Git integration — wraps GitWorker behind IPC and publishes bus events
    // (branch_changed, commit, dirty_changed) for any plugin or UI that
    // subscribes to `com.nexus.git.*`.
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
                &[
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
                ],
            ),
            forge_root,
            Box::new(GitCorePlugin::new(
                forge_root.to_path_buf(),
                Some(Arc::clone(event_bus)),
            )),
        )
        .context("failed to register com.nexus.git")?;

    // Terminal & process manager — PRD-09. Pure-library crate wrapped
    // behind `com.nexus.terminal` so UI / script plugins reach it over
    // dispatch rather than linking it directly (ARCHITECTURE §7 invariant #3).
    // Saved-commands (§14.1) are persisted via `SqliteSavedCommandStore`
    // at `<forge>/.forge/procmgr.sqlite`; failure to open the store is
    // logged and the plugin loads without saved-command handlers
    // (session IPC stays usable even when SQLite misbehaves).
    let saved_db = forge_root.join(".forge").join("procmgr.sqlite");
    let terminal_plugin = match nexus_terminal::SqliteSavedCommandStore::open(&saved_db) {
        Ok(store) => TerminalCorePlugin::new().with_saved_store(store),
        Err(err) => {
            tracing::warn!(
                path = %saved_db.display(),
                err = %err,
                "com.nexus.terminal: saved-commands store unavailable; handlers will return errors"
            );
            TerminalCorePlugin::new()
        }
    };
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.terminal",
                "Terminal",
                LifecycleFlags::NONE,
                &[
                    (
                        "create_session",
                        nexus_terminal::HANDLER_CREATE_SESSION,
                    ),
                    (
                        "close_session",
                        nexus_terminal::HANDLER_CLOSE_SESSION,
                    ),
                    ("send_input", nexus_terminal::HANDLER_SEND_INPUT),
                    (
                        "send_raw_input",
                        nexus_terminal::HANDLER_SEND_RAW_INPUT,
                    ),
                    ("pump", nexus_terminal::HANDLER_PUMP),
                    ("read_output", nexus_terminal::HANDLER_READ_OUTPUT),
                    (
                        "search_output",
                        nexus_terminal::HANDLER_SEARCH_OUTPUT,
                    ),
                    (
                        "wait_for_pattern",
                        nexus_terminal::HANDLER_WAIT_FOR_PATTERN,
                    ),
                    (
                        "get_session_info",
                        nexus_terminal::HANDLER_GET_SESSION_INFO,
                    ),
                    (
                        "list_sessions",
                        nexus_terminal::HANDLER_LIST_SESSIONS,
                    ),
                    (
                        "saved_list",
                        nexus_terminal::HANDLER_SAVED_LIST,
                    ),
                    (
                        "saved_create",
                        nexus_terminal::HANDLER_SAVED_CREATE,
                    ),
                    (
                        "saved_update",
                        nexus_terminal::HANDLER_SAVED_UPDATE,
                    ),
                    (
                        "saved_delete",
                        nexus_terminal::HANDLER_SAVED_DELETE,
                    ),
                    (
                        "saved_reorder",
                        nexus_terminal::HANDLER_SAVED_REORDER,
                    ),
                ],
            ),
            forge_root,
            Box::new(terminal_plugin),
        )
        .context("failed to register com.nexus.terminal")?;

    Ok(())
}

#[derive(Clone, Copy)]
struct LifecycleFlags {
    on_init: bool,
    on_start: bool,
    on_stop: bool,
}

impl LifecycleFlags {
    const NONE: Self = Self {
        on_init: false,
        on_start: false,
        on_stop: false,
    };
}

/// Generate a core-plugin manifest inline with no IPC commands declared.
fn core_manifest(id: &str, name: &str, lc: LifecycleFlags) -> PluginManifest {
    core_manifest_with_ipc(id, name, lc, &[])
}

/// Generate a core-plugin manifest with IPC command registrations.
fn core_manifest_with_ipc(
    id: &str,
    name: &str,
    lc: LifecycleFlags,
    ipc_commands: &[(&str, u32)],
) -> PluginManifest {
    let mut toml = format!(
        r#"
[plugin]
id = "{id}"
name = "{name}"
version = "0.1.0"
trust_level = "core"
api_version = "1"

[lifecycle]
on_init = {init}
on_start = {start}
on_stop = {stop}
"#,
        init = lc.on_init,
        start = lc.on_start,
        stop = lc.on_stop,
    );
    for (cmd_id, handler_id) in ipc_commands {
        toml.push_str(&format!(
            "\n[[registrations.ipc_command]]\nid = \"{cmd_id}\"\nhandler_id = {handler_id}\n"
        ));
    }
    parse_manifest(&toml, "bootstrap.toml")
        .unwrap_or_else(|e| panic!("bootstrap manifest for {id} failed to parse: {e}"))
}

fn invoker_manifest(id: &str, name: &str) -> PluginManifest {
    core_manifest(id, name, LifecycleFlags::NONE)
}

/// Placeholder CorePlugin for the CLI/TUI invoker. Neither invoker receives
/// IPC calls (they only originate them), so `dispatch` is never expected to
/// be called; we return an error if it somehow is.
struct InvokerPlugin {
    id: String,
}

impl CorePlugin for InvokerPlugin {
    fn dispatch(
        &mut self,
        handler_id: u32,
        _args: &serde_json::Value,
    ) -> std::result::Result<serde_json::Value, PluginError> {
        Err(PluginError::ExecutionFailed {
            plugin_id: self.id.clone(),
            reason: format!("invoker plugin has no IPC handlers (handler_id={handler_id})"),
        })
    }
}

// Silence the unused-import warning from explicitly re-exported kernel types.
const _: fn() = || {
    let _: Option<KernelPluginError> = None;
};
