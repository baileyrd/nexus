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
// The `database::*`, `storage::*`, `terminal::*`, `agent::*` modules are
// thin pass-through wrappers over `ipc_call(...)` and all fail for the
// same reasons (kernel IPC dispatch failure, plugin not loaded, response
// decode error). Documenting each individually would be 27 copies of the
// same paragraph.
#![allow(clippy::missing_errors_doc)]

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
/// Re-exported from `nexus-formats` (pure format library, no `SQLite`).
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
/// Plugin id for the in-tree Nexus gpui desktop shell invoker (ADR 0026).
pub const GPUI_PLUGIN_ID: &str = "com.nexus.gpui";

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
    ///
    /// # When to reach for this
    ///
    /// Issue #83. The audit suggested `pub(crate)` or "add accessors"
    /// — neither works in practice today. The shell's bridge
    /// (`shell/src-tauri/src/bridge.rs`) drains the loader during
    /// `shutdown_kernel` (every plugin's `on_stop` hook needs to
    /// fire), and the bootstrap's own integration tests reach for
    /// the loader as `Arc<dyn IpcDispatcher>` to spin up
    /// per-call `KernelPluginContext`s. Both are legitimate
    /// host-side uses that don't fit the `ipc_call` model.
    ///
    /// **For everything else, route through
    /// [`Runtime::context`]** — community / first-party plugin
    /// code, CLI commands, anything that the shell/CLI/TUI binary
    /// itself exposes to plugins. Direct loader access is for
    /// host-internal lifecycle plumbing, not for ergonomic
    /// shortcuts past the IPC layer.
    pub loader: Arc<SharedPluginLoader>,
}

/// Build a runtime with the CLI registered as the invoker plugin.
///
/// # Errors
/// Returns an error if the forge cannot be opened, the kernel config cannot
/// be loaded, or any core plugin fails to register or initialize.
// Accepts an owned PathBuf for ergonomic continuity with its ~60 callsites
// (CLI/TUI binaries + integration test suites). Switching to &Path would
// be a cross-crate API break without changing the implementation.
#[allow(clippy::needless_pass_by_value)]
pub fn build_cli_runtime(forge_root: PathBuf) -> Result<Runtime> {
    build(&forge_root, CLI_PLUGIN_ID, "Nexus CLI")
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
// See `build_cli_runtime` for the rationale on keeping PathBuf.
#[allow(clippy::needless_pass_by_value)]
pub fn build_tui_runtime(forge_root: PathBuf) -> Result<Runtime> {
    build(&forge_root, TUI_PLUGIN_ID, "Nexus TUI")
}

/// Build a runtime with the gpui desktop shell registered as the invoker plugin.
///
/// Called once at application startup by `nexus-gpui` before the gpui event
/// loop starts. The returned [`Runtime`] should be wrapped in
/// `Arc<Mutex<Runtime>>` and shared with gpui background tasks that make
/// `ipc_call`s via a `tokio::runtime::Runtime::block_on` bridge.
///
/// # Errors
/// See [`build_cli_runtime`].
#[allow(clippy::needless_pass_by_value)]
pub fn build_gpui_runtime(forge_root: PathBuf) -> Result<Runtime> {
    build(&forge_root, GPUI_PLUGIN_ID, "Nexus")
}

fn build(forge_root: &std::path::Path, invoker_id: &'static str, invoker_name: &str) -> Result<Runtime> {
    let config = KernelConfig::load(forge_root)
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
    register_core_plugins(&mut loader, forge_root, &event_bus)?;

    // Register the invoker (CLI or TUI) as a Core plugin so it holds a real
    // plugin identity with Capability::ALL.
    let invoker_manifest = invoker_manifest(invoker_id, invoker_name);
    loader
        .register_core(
            invoker_manifest,
            forge_root,
            Box::new(InvokerPlugin {
                id: invoker_id.to_string(),
            }),
        )
        .map_err(|e| anyhow::anyhow!("failed to register invoker plugin {invoker_id}: {e}"))?;

    let shared = Arc::new(SharedPluginLoader::new(loader));

    // Per-(target, command) capability gate (issue #77). Both of these
    // handlers spawn arbitrary processes — terminal `create_session`
    // takes a guest-supplied `shell` + `working_dir` + `env`, MCP
    // `connect` spawns the MCP server's `command` over stdio. Pre-#77
    // any caller holding `IpcCall` could reach them, laundering the
    // effect of `ProcessSpawn` through `IpcCall`. Now the kernel
    // context's `ipc_call` denies the dispatch unless the caller also
    // holds `ProcessSpawn`. Combined with #73 (workflow/agent contexts
    // dropped from `Capability::ALL`), this means a `.workflows/*.toml`
    // step or LLM-generated tool call can no longer escalate through
    // these surfaces.
    shared.add_cap_requirement(
        "com.nexus.terminal",
        "create_session",
        vec![Capability::ProcessSpawn],
    );
    shared.add_cap_requirement(
        "com.nexus.mcp.host",
        "connect",
        vec![Capability::ProcessSpawn],
    );

    // Per-handler ai.* capability gates per ADR 0022. Closes the AI
    // audit's §4 finding ("any caller with ipc.call can invoke any AI
    // handler"). Read-only handlers (status, config, index_status,
    // vectorstore_count, activity_list, apply) keep the ipc.call-only
    // default — they're either inert or already gated downstream
    // (apply writes via storage, which has its own fs.write check).
    for (cmd, cap) in [
        ("stream_chat", Capability::AiChat),
        ("stream_ask", Capability::AiChat),
        ("ask", Capability::AiChat),
        ("semantic_search", Capability::AiChat),
        ("enrich_file", Capability::AiChat),
        ("propose_tool_calls", Capability::AiChat),
        ("index_file", Capability::AiIndex),
        ("index_trigger", Capability::AiIndex),
        ("session_load", Capability::AiSessionRead),
        ("session_list", Capability::AiSessionRead),
        ("session_save", Capability::AiSessionWrite),
        ("session_delete", Capability::AiSessionWrite),
        ("set_config", Capability::AiConfigWrite),
        ("activity_clear", Capability::AiActivityWrite),
    ] {
        shared.add_cap_requirement("com.nexus.ai", cmd, vec![cap]);
    }

    // ADR 0022 Phase 2 — args-aware tool-policy enforcement. A
    // caller that requests `tools=auto` (the default) needs
    // `ai.tools.write` because the registry includes `write_file`;
    // `auto_with_mcp` additionally needs `ai.tools.mcp`. Read-only
    // (`tools=auto_readonly`) and no-tools paths add nothing on top
    // of `ai.chat`. Both `stream_chat` and `propose_tool_calls`
    // honour the same field, so they share the closure.
    let tool_policy_fn: nexus_plugins::CapRequirementFn = Arc::new(|args: &serde_json::Value| {
        // The `tools` field is optional and defaults to Auto. Both
        // arg envelopes carry `tools` at the top level with the
        // same shape, so a permissive lookup works for both.
        let policy = args
            .get("tools")
            .and_then(|v| serde_json::from_value::<nexus_ai::ipc::AiToolPolicy>(v.clone()).ok())
            .unwrap_or_default();
        nexus_ai::ipc::extra_caps_for_policy(policy)
    });
    shared.add_cap_requirement_fn("com.nexus.ai", "stream_chat", Arc::clone(&tool_policy_fn));
    shared.add_cap_requirement_fn(
        "com.nexus.ai",
        "propose_tool_calls",
        Arc::clone(&tool_policy_fn),
    );

    // ADR 0024 Phase 2a — agent session_run drives the same
    // tool-loop machinery as stream_chat, just with approval policy
    // injected. Gate on ai.chat (consistent with propose_tool_calls)
    // so a caller without it can't reach session_run either.
    shared.add_cap_requirement(
        "com.nexus.agent",
        "session_run",
        vec![Capability::AiChat],
    );
    // round_decide is the caller's reply to a round_proposed event;
    // mirroring the cap on session_run keeps the surface consistent
    // (a caller without ai.chat couldn't have started the session
    // anyway, but pinning the gate avoids a future regression where
    // a bystander plugin pushes decisions into someone else's session).
    shared.add_cap_requirement(
        "com.nexus.agent",
        "round_decide",
        vec![Capability::AiChat],
    );

    let dispatcher: Arc<dyn IpcDispatcher> = Arc::clone(&shared) as Arc<dyn IpcDispatcher>;

    // Hand the AI plugin its own KernelPluginContext so `ask`/`index_file`
    // handlers can issue nested `ipc_call`s into storage through the same
    // plugin-facing surface the invoker uses — no raw-dispatcher leak.
    let ai_ctx = KernelPluginContext::new(
        "com.nexus.ai",
        env!("CARGO_PKG_VERSION"),
        all_caps(),
        Arc::clone(&kv_store),
        Arc::clone(&event_bus),
        forge_root,
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
    //
    // Capability scope (#73): the agent's direct-cap usage is
    // `IpcCall` (tool dispatch via ipc_call to AI/storage/etc.) and
    // `FsRead` (history file reads at
    // `crates/nexus-agent/src/core_plugin.rs:517`). It does NOT
    // directly write files, fetch URLs, or spawn processes — those
    // come transitively through ipc_call to the relevant plugin and
    // are gated by *that* plugin's capability checks. Granting only
    // the directly-used caps makes the contract truthful and
    // prevents silent escalation if new direct-cap code is added.
    // The IpcCall-laundering problem (workflow/agent can still
    // ipc_call into high-impact handlers like terminal::create_session)
    // is tracked separately under #77.
    let agent_ctx = KernelPluginContext::new(
        "com.nexus.agent",
        env!("CARGO_PKG_VERSION"),
        agent_capabilities(),
        Arc::clone(&kv_store),
        Arc::clone(&event_bus),
        forge_root,
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
        all_caps(),
        Arc::clone(&kv_store),
        Arc::clone(&event_bus),
        forge_root,
        Some(Arc::clone(&dispatcher)),
    )
    .context("failed to build kernel plugin context for com.nexus.editor")?;
    shared
        .wire_context("com.nexus.editor", Arc::new(editor_ctx))
        .map_err(|e| anyhow::anyhow!("failed to wire editor plugin context: {e}"))?;

    // Workflow plugin needs the kernel context so its `run` handler
    // can drive arbitrary plugins via ipc_call.
    //
    // Capability scope (#73): the workflow plugin's direct-cap usage
    // is just `IpcCall` — every file/network/process operation it
    // performs (digest scheduler reads, AI-prompt steps, storage
    // writes) routes through `ctx.ipc_call(...)`, gated by the target
    // plugin's own capability checks. Granting only `IpcCall` makes
    // the contract truthful for both the on-demand `workflow::run`
    // handler and the cron-driven digest scheduler that runs on top
    // of the same context. The IpcCall-laundering problem
    // (workflow can still ipc_call into terminal::create_session,
    // mcp::connect, etc.) is tracked separately under #77.
    let workflow_ctx = KernelPluginContext::new(
        "com.nexus.workflow",
        env!("CARGO_PKG_VERSION"),
        workflow_capabilities(),
        Arc::clone(&kv_store),
        Arc::clone(&event_bus),
        forge_root,
        Some(Arc::clone(&dispatcher)),
    )
    .context("failed to build kernel plugin context for com.nexus.workflow")?;
    shared
        .wire_context("com.nexus.workflow", Arc::new(workflow_ctx))
        .map_err(|e| anyhow::anyhow!("failed to wire workflow plugin context: {e}"))?;

    let context = KernelPluginContext::new(
        invoker_id,
        env!("CARGO_PKG_VERSION"),
        all_caps(),
        kv_store,
        event_bus,
        forge_root,
        Some(dispatcher),
    )
    .context("failed to build kernel plugin context for invoker")?;

    Ok(Runtime {
        kernel,
        context,
        loader: shared,
    })
}

#[allow(clippy::too_many_lines)]
fn register_core_plugins(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    use nexus_agent::AgentCorePlugin;
    use nexus_ai::AiCorePlugin;
    use nexus_comments::core_plugin::CommentsCorePlugin;
    use nexus_linkpreview::core_plugin::LinkPreviewCorePlugin;
    use nexus_formats::FormatsCorePlugin;
    use nexus_skills::SkillsCorePlugin;
    use nexus_templates::TemplatesCorePlugin;
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

    // Storage is the pilot for ADR 0021 (handler versioning). Every
    // command below is registered under both `<command>` and
    // `<command>.v1` via `with_v1_aliases` — the bare alias tracks the
    // current version, the `.v1` form is the explicit pin. Existing
    // callers using the bare names continue to work unchanged.
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
                ],
            ),
            forge_root,
            Box::new(EditorCorePlugin::with_event_bus(
                forge_root.to_path_buf(),
                Arc::clone(event_bus),
            )),
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
                    // BL-041 — gracefully tear down the background
                    // indexing daemon on shutdown. (`on_start` stays
                    // false; the daemon is spawned from
                    // `wire_context` because that's the first hook
                    // with the kernel context in hand.)
                    on_stop: true,
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
                    (
                        "set_config",
                        nexus_ai::core_plugin::HANDLER_SET_CONFIG,
                    ),
                    (
                        "semantic_search",
                        nexus_ai::core_plugin::HANDLER_SEMANTIC_SEARCH,
                    ),
                    // BL-041 — background indexing daemon status
                    // snapshot. Polled by the shell status badge
                    // (~2 s cadence) and surfaced through `nexus
                    // status` for headless use.
                    (
                        "index_status",
                        nexus_ai::core_plugin::HANDLER_INDEX_STATUS,
                    ),
                    // BL-045 — auto-enrichment on save. `enrich_file`
                    // proposes tags + summary + related notes for a
                    // markdown file (no write); `enrich_apply` merges
                    // a previously-returned proposal into the file's
                    // YAML frontmatter (with a body-hash drift guard).
                    (
                        "enrich_file",
                        nexus_ai::core_plugin::HANDLER_ENRICH_FILE,
                    ),
                    (
                        "enrich_apply",
                        nexus_ai::core_plugin::HANDLER_ENRICH_APPLY,
                    ),
                    // FU-2 — manual "Reindex forge" trigger. Fans
                    // every markdown file currently in the storage
                    // index onto the indexing daemon's queue. Used
                    // by the shell's status badge button + palette
                    // command.
                    (
                        "index_trigger",
                        nexus_ai::core_plugin::HANDLER_INDEX_TRIGGER,
                    ),
                    // BL-037 — per-forge AI activity timeline.
                    // `activity_list` reads the JSONL log under
                    // `.forge/ai-activity.log` newest-first;
                    // `activity_clear` truncates it.
                    (
                        "activity_list",
                        nexus_ai::core_plugin::HANDLER_ACTIVITY_LIST,
                    ),
                    (
                        "activity_clear",
                        nexus_ai::core_plugin::HANDLER_ACTIVITY_CLEAR,
                    ),
                    // G7 / ADR 0023 — single-turn provider call that
                    // returns mapped tool-use blocks without executing
                    // them. Consumed by the agent migration (Phase 1b);
                    // exposed here so the IPC contract is reachable
                    // independently of caller wiring.
                    (
                        "propose_tool_calls",
                        nexus_ai::core_plugin::HANDLER_PROPOSE_TOOL_CALLS,
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
                    ("compose", nexus_skills::HANDLER_COMPOSE),
                ],
            ),
            forge_root,
            Box::new(SkillsCorePlugin::open(skills_dir)),
        )
        .context("failed to register com.nexus.skills")?;

    // Templates — page-template subsystem. Holds the forge root and
    // serves list/get/render/apply/reload over IPC. Built-ins are
    // included automatically; user templates live at
    // `<forge>/.forge/templates/`.
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.templates",
                "Templates",
                LifecycleFlags::NONE,
                &[
                    ("list", nexus_templates::HANDLER_LIST),
                    ("get", nexus_templates::HANDLER_GET),
                    ("render", nexus_templates::HANDLER_RENDER),
                    ("apply", nexus_templates::HANDLER_APPLY),
                    ("reload", nexus_templates::HANDLER_RELOAD),
                ],
            ),
            forge_root,
            Box::new(TemplatesCorePlugin::open(forge_root.to_path_buf())),
        )
        .context("failed to register com.nexus.templates")?;

    // Formats — Notion zip-import / format-export. Wraps the
    // pure-library converters in `nexus-formats::notion` behind two
    // IPC handlers so the shell, CLI plugins, and external clients can
    // drive imports/exports through one path.
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.formats",
                "Formats",
                LifecycleFlags::NONE,
                &[
                    ("import_notion", nexus_formats::HANDLER_IMPORT_NOTION),
                    ("export_notion", nexus_formats::HANDLER_EXPORT_NOTION),
                ],
            ),
            forge_root,
            Box::new(FormatsCorePlugin::open(forge_root.to_path_buf())),
        )
        .context("failed to register com.nexus.formats")?;

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
                    ("run_digest", nexus_workflow::HANDLER_RUN_DIGEST),
                    // FU-7 — live config push. Lets the shell flip
                    // [digests].enabled / cron strings without
                    // restarting the kernel.
                    (
                        "set_digest_config",
                        nexus_workflow::HANDLER_SET_DIGEST_CONFIG,
                    ),
                    // BL-028f — built-in templates library.
                    (
                        "templates_list",
                        nexus_workflow::core_plugin::HANDLER_TEMPLATES_LIST,
                    ),
                    (
                        "templates_get",
                        nexus_workflow::core_plugin::HANDLER_TEMPLATES_GET,
                    ),
                    (
                        "templates_init",
                        nexus_workflow::core_plugin::HANDLER_TEMPLATES_INIT,
                    ),
                ],
            ),
            forge_root,
            Box::new(WorkflowCorePlugin::open_full(
                workflows_dir,
                load_digest_config(forge_root),
                load_webhook_config(forge_root),
            )),
        )
        .context("failed to register com.nexus.workflow")?;

    // Link preview — outbound HTTP fetcher that backs the canvas
    // link-node overlay in the shell. Stateless; the shell owns the
    // cache. Fetches are best-effort and time-bounded (5 s) so a
    // slow host can't hang a canvas render.
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.linkpreview",
                "Link preview",
                LifecycleFlags::NONE,
                &[("fetch", nexus_linkpreview::core_plugin::HANDLER_FETCH)],
            ),
            forge_root,
            Box::new(LinkPreviewCorePlugin::new()),
        )
        .context("failed to register com.nexus.linkpreview")?;

    // Comments — BL-050. Side-margin comment threads anchored to
    // stable block ids (ADR 0017). Storage in
    // `<forge>/.forge/comments/<relpath>.json`. Stateless: every
    // dispatch hits disk fresh.
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.comments",
                "Comments",
                LifecycleFlags::NONE,
                &[
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
                ],
            ),
            forge_root,
            Box::new(CommentsCorePlugin::new(forge_root)),
        )
        .context("failed to register com.nexus.comments")?;

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
                    ("history_list", nexus_agent::HANDLER_HISTORY_LIST),
                    ("history_get", nexus_agent::HANDLER_HISTORY_GET),
                    ("history_delete", nexus_agent::HANDLER_HISTORY_DELETE),
                    ("list_archetypes", nexus_agent::HANDLER_LIST_ARCHETYPES),
                    // ADR 0024 Phase 2a — agent session tool-loop.
                    ("session_run", nexus_agent::core_plugin::HANDLER_SESSION_RUN),
                    ("session_list", nexus_agent::core_plugin::HANDLER_SESSION_LIST),
                    ("session_get", nexus_agent::core_plugin::HANDLER_SESSION_GET),
                    ("session_delete", nexus_agent::core_plugin::HANDLER_SESSION_DELETE),
                    // ADR 0024 Phase 2b — caller-side approval reply.
                    ("round_decide", nexus_agent::core_plugin::HANDLER_ROUND_DECIDE),
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
    // Phase 2 WI-12: stream PTY output as kernel events so the shell
    // can switch off its 100ms poll. The legacy `pump` handler still
    // returns its byte count; this is purely additive.
    let terminal_plugin = terminal_plugin.with_event_bus(Arc::clone(event_bus));
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
                        "read_raw_since",
                        nexus_terminal::HANDLER_READ_RAW_SINCE,
                    ),
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
    let no_commands: &[(&str, u32)] = &[];
    core_manifest_with_ipc(id, name, lc, no_commands)
}

/// Generate a core-plugin manifest with IPC command registrations.
///
/// Generic over the command-name string type so the same builder accepts
/// both the static `&str` slices used by most subsystems and the owned
/// `String`s produced by [`with_v1_aliases`] (ADR 0021).
fn core_manifest_with_ipc<S: AsRef<str>>(
    id: &str,
    name: &str,
    lc: LifecycleFlags,
    ipc_commands: &[(S, u32)],
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
        use std::fmt::Write as _;
        let cmd_id = cmd_id.as_ref();
        let _ = write!(
            toml,
            "\n[[registrations.ipc_command]]\nid = \"{cmd_id}\"\nhandler_id = {handler_id}\n"
        );
    }
    parse_manifest(&toml, "bootstrap.toml")
        .unwrap_or_else(|e| panic!("bootstrap manifest for {id} failed to parse: {e}"))
}

/// Expand a list of `(command, handler_id)` pairs to include `.v1`
/// aliases per [ADR 0021](../../../docs/adr/0021-ipc-handler-versioning.md).
///
/// For `[("search", 7)]` returns `[("search", 7), ("search.v1", 7)]`.
/// Both names resolve to the same handler — the bare form is the
/// "current version" alias and `.v1` is the explicit version pin. When
/// `search.v2` ships, the subsystem switches to a hand-written list that
/// carries all three names (bare → v2's handler, `.v1` → legacy
/// handler, `.v2` → new handler) so the deprecation timeline is visible
/// at the registration site.
pub(crate) fn with_v1_aliases(ipc_commands: &[(&str, u32)]) -> Vec<(String, u32)> {
    let mut out = Vec::with_capacity(ipc_commands.len() * 2);
    for &(name, handler_id) in ipc_commands {
        out.push((name.to_string(), handler_id));
        out.push((format!("{name}.v1"), handler_id));
    }
    out
}

fn invoker_manifest(id: &str, name: &str) -> PluginManifest {
    core_manifest(id, name, LifecycleFlags::NONE)
}

/// Placeholder `CorePlugin` for the CLI/TUI invoker. Neither invoker receives
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

/// Load the BL-047 `[digests]` block from `<forge>/.forge/config.toml`.
///
/// Missing file, missing block, or any parse / IO error all fall back
/// to [`nexus_workflow::DigestConfig::default`] (digests disabled).
/// This keeps existing forges working with no config changes.
fn load_digest_config(forge_root: &std::path::Path) -> nexus_workflow::DigestConfig {
    #[derive(serde::Deserialize)]
    struct Wrapper {
        #[serde(default)]
        digests: Option<nexus_workflow::DigestConfig>,
    }
    let path = forge_root.join(".forge").join("config.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        // Missing config is the normal first-boot path — fall back
        // silently to defaults. Any other read error (permission
        // flip, I/O failure on the forge volume, …) needs to be
        // visible so the operator can investigate; falling back to
        // defaults is the safe behaviour but the warn logs the
        // signal. See issue #83.
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return nexus_workflow::DigestConfig::default();
        }
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                %err,
                "config.toml: read failed; falling back to default [digests] config"
            );
            return nexus_workflow::DigestConfig::default();
        }
    };
    match toml::from_str::<Wrapper>(&text) {
        Ok(w) => w.digests.unwrap_or_default(),
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                %err,
                "config.toml: [digests] failed to parse; falling back to defaults"
            );
            nexus_workflow::DigestConfig::default()
        }
    }
}

/// `Capability::ALL` collected into a `CapabilitySet`. Used by the
/// AI / editor / shell-invoker contexts that legitimately need
/// every capability — the workflow and agent contexts have their
/// own narrower sets (see [`agent_capabilities`],
/// [`workflow_capabilities`]) per #73. Extracted from five inline
/// repetitions across the bootstrap path; see issue #83.
///
/// "Do all five contexts truly need every capability?" was a
/// reasonable question from the audit. After #73 only three contexts
/// hold the full set today; the remaining call sites all wire the
/// same shape so the helper centralises the "I really do need
/// everything" decision rather than re-deriving it inline.
#[must_use]
pub fn all_caps() -> CapabilitySet {
    Capability::ALL.iter().copied().collect()
}

/// Capabilities granted to the `com.nexus.agent` `KernelPluginContext`
/// at runtime wiring time (issue #73). Scoped to the agent's actual
/// direct-cap usage:
///
/// - `IpcCall` — every `ToolCall` from a plan dispatches via
///   `ctx.ipc_call(target_plugin_id, command_id, …)` (see
///   `crates/nexus-bootstrap/src/agent.rs:62-73`), and the planning
///   loop uses `ipc_call` against `com.nexus.ai::stream_chat`.
/// - `FsRead` — `crates/nexus-agent/src/core_plugin.rs:517` reads
///   plan-history JSON files directly via `ctx.read_file(…)`.
/// - `FsWrite` — `crates/nexus-agent/src/core_plugin.rs:580` deletes
///   one persisted history entry via `ctx.delete_file(…)` (the
///   `history_delete` handler). Asymmetric with the history-save
///   path, which routes through `com.nexus.storage::write_file` via
///   `ipc_call`; routing the delete the same way is a clean
///   follow-up but out of scope for #73.
///
/// Pre-#73 this was `Capability::ALL`; the audit's amplifier-plugin
/// finding is that an LLM-generated plan or an attacker-influenced
/// prompt could exercise NetHttp / ProcessSpawn / FsReadExternal /
/// FsWriteExternal directly from the agent's context. Restricting to
/// the directly-used set prevents silent escalation if new
/// direct-cap code is added. `FsRead` and `FsWrite` are confined to
/// the forge root by the kernel's `confine_path` (`context_impl.rs`),
/// so they don't grant external filesystem access; the
/// transitive IpcCall-laundering surface (agent → terminal,
/// agent → mcp, …) is tracked under #77.
#[must_use]
pub fn agent_capabilities() -> CapabilitySet {
    [
        Capability::IpcCall,
        Capability::FsRead,
        Capability::FsWrite,
        // ADR 0022 Phase 1: planner LLM calls go through
        // `com.nexus.ai::propose_tool_calls`, which requires
        // `ai.chat`. No `ai.config.write` / `ai.activity.write` so
        // a manipulated plan can't rotate credentials or wipe the
        // audit log.
        Capability::AiChat,
        // ADR 0022 Phase 2: planner advertises `write_file` so the
        // model can propose write tool-use blocks (executed under
        // PlanExecutor's per-step approval policy, not by the AI
        // tool-loop). Without this the agent could only ever
        // produce read-only plans. No `ai.tools.mcp` — MCP reach is
        // opt-in per call.
        Capability::AiToolsWrite,
    ]
    .into_iter()
    .collect()
}

/// Capabilities granted to the `com.nexus.workflow` `KernelPluginContext`
/// at runtime wiring time (issue #73). Scoped to `IpcCall` only —
/// every step type in the workflow executor (ipc/ipc_call, ai_prompt,
/// digest reads/writes, …) routes through `ctx.ipc_call(…)` rather
/// than calling kernel surfaces directly.
///
/// Pre-#73 this was `Capability::ALL`; user-authored `.workflows/*.toml`
/// drives the steps, so this is exactly the
/// "amplifier plugin gets everything" pattern the audit calls out.
/// The cron-driven digest scheduler runs on top of the same context,
/// so this scope applies there too. Same caveat about the transitive
/// IpcCall-laundering surface as for `agent_capabilities`.
#[must_use]
pub fn workflow_capabilities() -> CapabilitySet {
    // ADR 0022: workflow `ai_prompt` steps reach `stream_chat` via
    // `ipc_call`, which now requires `ai.chat`. No write/config/activity
    // ai.* caps so a user-authored workflow step can't rotate
    // credentials or wipe the audit log.
    [Capability::IpcCall, Capability::AiChat]
        .into_iter()
        .collect()
}

/// BL-028g — pull `[webhooks]` out of `<forge>/.forge/config.toml`.
/// Same fallback behaviour as [`load_digest_config`].
fn load_webhook_config(forge_root: &std::path::Path) -> nexus_workflow::webhook::WebhookConfig {
    #[derive(serde::Deserialize)]
    struct Wrapper {
        #[serde(default)]
        webhooks: Option<nexus_workflow::webhook::WebhookConfig>,
    }
    let path = forge_root.join(".forge").join("config.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return nexus_workflow::webhook::WebhookConfig::default();
        }
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                %err,
                "config.toml: read failed; falling back to default [webhooks] config"
            );
            return nexus_workflow::webhook::WebhookConfig::default();
        }
    };
    match toml::from_str::<Wrapper>(&text) {
        Ok(w) => w.webhooks.unwrap_or_default(),
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                %err,
                "config.toml: [webhooks] failed to parse; falling back to defaults"
            );
            nexus_workflow::webhook::WebhookConfig::default()
        }
    }
}

#[cfg(test)]
mod with_v1_aliases_tests {
    use super::with_v1_aliases;

    #[test]
    fn doubles_each_entry_with_v1_suffix() {
        let expanded = with_v1_aliases(&[("search", 7), ("read_file", 2)]);
        assert_eq!(
            expanded,
            vec![
                ("search".to_string(), 7),
                ("search.v1".to_string(), 7),
                ("read_file".to_string(), 2),
                ("read_file.v1".to_string(), 2),
            ],
            "every input pair must produce a bare alias and a .v1 alias"
        );
    }

    #[test]
    fn empty_input_yields_empty_output() {
        assert!(with_v1_aliases(&[]).is_empty());
    }

    #[test]
    fn handler_id_is_shared_between_bare_and_v1() {
        let expanded = with_v1_aliases(&[("delete_file", 12)]);
        let bare = expanded.iter().find(|(n, _)| n == "delete_file");
        let v1 = expanded.iter().find(|(n, _)| n == "delete_file.v1");
        assert_eq!(
            bare.map(|(_, h)| *h),
            v1.map(|(_, h)| *h),
            "bare and .v1 must point at the same handler id (alias semantics)"
        );
    }
}
