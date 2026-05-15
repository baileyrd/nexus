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
pub mod forge_template;
mod audit_sqlite;
pub mod crdt_publisher;
pub mod database;
pub mod dream_cycle;
pub mod storage;
pub mod terminal;
/// BL-113 / ADR 0027 — manifest-side `ContributedAdapter` → host-side
/// `{Lsp,Mcp}ServerSpec` converters. Phase 2a/3a primitive; the
/// bootstrap-side wiring that calls these converters and feeds the
/// result through `merge_contributed` lands in Phase 2b/3b once the
/// plugin-lifecycle callback shape is settled by Phase 1.
pub mod protocol_host_specs;

/// BL-113 Phase 1c — bootstrap-side wiring that issues
/// `com.nexus.dap::register_adapter` IPC calls for each DAP
/// contribution found across a set of community plugin manifests,
/// and unwires them at plugin disable / shutdown.
pub mod dap_contribution_wiring;

/// BL-113 Phase 2b — bootstrap-side wiring that issues
/// `com.nexus.lsp::register_server` IPC calls for each LSP
/// contribution found across a set of community plugin manifests,
/// and unwires them at plugin disable / shutdown.
pub mod lsp_contribution_wiring;

/// BL-113 Phase 3b — bootstrap-side wiring that issues
/// `com.nexus.mcp.host::register_server` IPC calls for each MCP
/// contribution found across a set of community plugin manifests,
/// and unwires them at plugin disable / shutdown.
pub mod mcp_contribution_wiring;

/// BL-113 Phase 4 / BL-144 — bootstrap-side wiring that issues
/// `com.nexus.acp::register_server` IPC calls for each ACP
/// contribution found across a set of community plugin manifests,
/// and unwires them at plugin disable / shutdown.
pub mod acp_contribution_wiring;

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

fn build(forge_root: &std::path::Path, invoker_id: &'static str, invoker_name: &str) -> Result<Runtime> {
    let config = KernelConfig::load(forge_root)
        .with_context(|| format!("failed to load kernel config at '{}'", forge_root.display()))?;

    // The KV store lives under `.forge/kv.sqlite3`. Create the dir here —
    // kernel is backend-agnostic now, so dir creation is bootstrap's job.
    let forge_dir = forge_root.join(".forge");
    std::fs::create_dir_all(&forge_dir)
        .with_context(|| format!("failed to create forge dir '{}'", forge_dir.display()))?;

    // BL-094: install the global audit-event store before any plugin
    // loads so capability grants/denials at boot are persisted.
    // Failure is logged but non-fatal — audit emission falls back to
    // tracing-only when the store is absent.
    let audit_path = forge_dir.join(".kernel").join("audit.db");
    if let Err(e) = audit_sqlite::init(&audit_path) {
        tracing::warn!(
            path = %audit_path.display(),
            error = %e,
            "audit store init failed; events will trace-only"
        );
    }

    let kv_path = forge_dir.join("kv.sqlite3");
    let kv_store: Arc<dyn nexus_kernel::KvStore> =
        Arc::new(nexus_kv::SqliteKvStore::open(&kv_path).with_context(|| {
            format!("failed to open kernel KV store at '{}'", kv_path.display())
        })?);

    // BL-093: install the global kernel-metrics registry before
    // any plugin lifecycle hooks fire so boot-time capability
    // grants and the security plugin's `on_init` already get
    // recorded.
    nexus_kernel::metrics::install(Arc::new(nexus_kernel::KernelMetrics::new()));

    let kernel = Kernel::new(config, kv_store).context("failed to build kernel")?;
    let event_bus: Arc<EventBus> = kernel.event_bus();
    let kv_store = kernel.kv_store();

    let plugins_dir = forge_root.join(".forge").join("plugins");
    let mut loader = PluginLoader::new(&plugins_dir);
    for extra in &kernel.config().plugin_search_paths {
        loader.add_search_path(extra.clone());
    }
    loader.set_event_bus(Arc::clone(&event_bus));
    loader.set_lifecycle_timeout(std::time::Duration::from_secs(
        kernel.config().lifecycle_timeout_secs,
    ));
    loader.set_require_signatures(kernel.config().require_signatures);

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
        // BL-129 Dream-Cycle phases — share the same AiChat gate as
        // enrich_file since they ultimately invoke the chat provider.
        ("enrich_entity", Capability::AiChat),
        ("infer_entity_relations", Capability::AiChat),
        ("propose_tool_calls", Capability::AiChat),
        // BL-116 — generate_docs dispatches a single-turn chat
        // completion, gated under the same ai.chat capability the
        // rest of the chat surface uses.
        ("generate_docs", Capability::AiChat),
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

    // BL-117 — audio subsystem caps. Microphone capture (transcribe)
    // is privacy-sensitive; speaker output (synthesize) is annoying
    // but not destructive. `status` is read-only and ungated so a
    // settings panel can probe the active backend pair without a cap
    // negotiation.
    shared.add_cap_requirement(
        "com.nexus.audio",
        "transcribe",
        vec![Capability::AudioRecord],
    );
    shared.add_cap_requirement(
        "com.nexus.audio",
        "synthesize",
        vec![Capability::AudioSynthesize],
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

    // BL-117 — audio subsystem's provider-routed backend issues
    // `ipc_call("com.nexus.ai", "resolve_credentials", …)` at
    // dispatch time, and reqwest-talks to the AI provider's audio
    // endpoint. Capabilities are scoped to those two surfaces; the
    // backend has no other direct kernel reach.
    let audio_ctx = KernelPluginContext::new(
        "com.nexus.audio",
        env!("CARGO_PKG_VERSION"),
        audio_capabilities(),
        Arc::clone(&kv_store),
        Arc::clone(&event_bus),
        forge_root,
        Some(Arc::clone(&dispatcher)),
    )
    .context("failed to build kernel plugin context for com.nexus.audio")?;
    shared
        .wire_context("com.nexus.audio", Arc::new(audio_ctx))
        .map_err(|e| anyhow::anyhow!("failed to wire audio plugin context: {e}"))?;

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

/// BL-095 follow-up — extension trait that converts a single
/// plugin's `LifecycleTimeout` from a fatal boot error into a
/// "skip and continue" signal. Other `register_core` errors
/// (manifest invalid, duplicate id, downstream lifecycle hook
/// returning a real error) still abort boot — the watchdog only
/// catches the case where a hook *hangs*, which is the recoverable
/// failure mode where the rest of the plugin set is still useful.
///
/// Each skip publishes `com.nexus.kernel.plugin_lifecycle_timeout`
/// onto the event bus so the shell (or any subscriber) can render
/// a "<plugin> failed to start" notice. The synthetic `com.nexus.kernel`
/// source-plugin-id is anchor-only — bus's namespace anti-spoof
/// check passes since the topic lies inside that string namespace.
trait RegisterCoreResultExt {
    fn or_lifecycle_skip(
        self,
        event_bus: &EventBus,
        label: &str,
    ) -> Result<()>;
}

impl RegisterCoreResultExt
    for std::result::Result<nexus_plugins::PluginInfo, PluginError>
{
    fn or_lifecycle_skip(
        self,
        event_bus: &EventBus,
        label: &str,
    ) -> Result<()> {
        match self {
            Ok(_) => Ok(()),
            Err(PluginError::LifecycleTimeout {
                plugin_id,
                hook,
                timeout_secs,
            }) => {
                tracing::warn!(
                    plugin_id = %plugin_id,
                    ?hook,
                    timeout_secs,
                    "BL-095: plugin lifecycle hook timed out — continuing with degraded plugin set",
                );
                let _ = event_bus.publish_plugin(
                    "com.nexus.kernel",
                    "com.nexus.kernel.plugin_lifecycle_timeout",
                    serde_json::json!({
                        "plugin_id": plugin_id,
                        "hook": format!("{:?}", hook),
                        "timeout_secs": timeout_secs,
                    }),
                );
                Ok(())
            }
            Err(e) => {
                Err(anyhow::Error::new(e).context(format!("failed to register {label}")))
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
fn register_core_plugins(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    use nexus_agent::AgentCorePlugin;
    use nexus_ai::AiCorePlugin;
    use nexus_audio::AudioCorePlugin;
    use nexus_comments::core_plugin::CommentsCorePlugin;
    use nexus_linkpreview::core_plugin::LinkPreviewCorePlugin;
    use nexus_notifications::core_plugin::NotificationsCorePlugin;
    use nexus_formats::FormatsCorePlugin;
    use nexus_skills::SkillsCorePlugin;
    use nexus_templates::TemplatesCorePlugin;
    use nexus_workflow::WorkflowCorePlugin;
    use nexus_editor::EditorCorePlugin;
    use nexus_git::GitCorePlugin;
    use nexus_lsp::LspCorePlugin;
    use nexus_dap::DapCorePlugin;
    use nexus_acp::AcpCorePlugin;
    use nexus_mcp::McpHostPlugin;
    use nexus_security::SecurityCorePlugin;
    use nexus_storage::{StorageConfig, StorageCorePlugin};
    use nexus_terminal::TerminalCorePlugin;
    use nexus_theme::ThemeCorePlugin;

    // Security first so audit events are available before other plugins emit.
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

    // `nexus-database` is a pure-logic library (types, validation, formulas,
    // CSV import/export). Its core plugin surfaces only those pure helpers
    // over IPC as `com.nexus.database`; SQL-backed base queries go through
    // `com.nexus.storage` (`base_index` / `base_list` / `base_query`) which
    // is the sole owner of the forge's SQLite database. See
    // docs/architecture/C4.md §4.2.
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.database",
                "Database",
                LifecycleFlags::NONE,
                &with_v1_aliases(&[
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
                    // DG-41 (PRD-10 §7) — relation resolution + rollup.
                    (
                        "resolve_relation",
                        nexus_database::core_plugin::HANDLER_RESOLVE_RELATION,
                    ),
                    (
                        "compute_rollup",
                        nexus_database::core_plugin::HANDLER_COMPUTE_ROLLUP,
                    ),
                ]),
            ),
            forge_root,
            Box::new(nexus_database::DatabaseCorePlugin::new()),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.database")?;

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
                &with_v1_aliases(&[
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
                ]),
            ),
            forge_root,
            Box::new(ThemeCorePlugin::with_builtins(Some(Arc::clone(event_bus)))),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.theme")?;

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
                &with_v1_aliases(&[
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
                    // BL-117 — sibling subsystems (nexus-audio) ask
                    // here for the active chat provider's credentials
                    // so the user doesn't need to configure a second
                    // API key for audio.
                    (
                        "resolve_credentials",
                        nexus_ai::core_plugin::HANDLER_RESOLVE_CREDENTIALS,
                    ),
                    // BL-116 — symbol-aware doc generator. Resolves a
                    // symbol from the BL-114 index, reads its source
                    // range, packs in parent + sibling 1-hop context
                    // (call-edges land in a follow-up BL), prompts
                    // the configured AI provider for a docblock.
                    (
                        "generate_docs",
                        nexus_ai::core_plugin::HANDLER_GENERATE_DOCS,
                    ),
                    // BL-128 close — `entity_recall`: FAISS-backed
                    // entity recall layered on the shared chunk
                    // vectorstore. Callers fall back to the
                    // substring-ranking `entity_search` when no
                    // embedder is configured.
                    (
                        "entity_recall",
                        nexus_ai::core_plugin::HANDLER_ENTITY_RECALL,
                    ),
                    (
                        "enrich_entity",
                        nexus_ai::core_plugin::HANDLER_ENRICH_ENTITY,
                    ),
                    (
                        "infer_entity_relations",
                        nexus_ai::core_plugin::HANDLER_INFER_ENTITY_RELATIONS,
                    ),
                ]),
            ),
            forge_root,
            Box::new(AiCorePlugin::new()),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.ai")?;

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
                &with_v1_aliases(&[
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
                    ("invoke", nexus_skills::HANDLER_INVOKE),
                ]),
            ),
            forge_root,
            Box::new(SkillsCorePlugin::open(skills_dir)),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.skills")?;

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
                &with_v1_aliases(&[
                    ("list", nexus_templates::HANDLER_LIST),
                    ("get", nexus_templates::HANDLER_GET),
                    ("render", nexus_templates::HANDLER_RENDER),
                    ("apply", nexus_templates::HANDLER_APPLY),
                    ("reload", nexus_templates::HANDLER_RELOAD),
                ]),
            ),
            forge_root,
            Box::new(TemplatesCorePlugin::open(forge_root.to_path_buf())),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.templates")?;

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
                &with_v1_aliases(&[
                    ("import_notion", nexus_formats::HANDLER_IMPORT_NOTION),
                    ("export_notion", nexus_formats::HANDLER_EXPORT_NOTION),
                ]),
            ),
            forge_root,
            Box::new(FormatsCorePlugin::open(forge_root.to_path_buf())),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.formats")?;

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
                &with_v1_aliases(&[
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
                    // BL-054 Phase 4 follow-up — persisted run history
                    // for the observability "Automation" tab.
                    (
                        "run_history",
                        nexus_workflow::HANDLER_RUN_HISTORY,
                    ),
                    // BL-054 Phase 4 follow-up — next-fire timestamp
                    // for cron-triggered workflows so the Automation
                    // tab can render an actual schedule preview.
                    (
                        "next_fire",
                        nexus_workflow::core_plugin::HANDLER_NEXT_FIRE,
                    ),
                ]),
            ),
            forge_root,
            Box::new(WorkflowCorePlugin::open_full(
                workflows_dir,
                load_digest_config(forge_root),
                load_webhook_config(forge_root),
            )),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.workflow")?;

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
                &with_v1_aliases(&[("fetch", nexus_linkpreview::core_plugin::HANDLER_FETCH)]),
            ),
            forge_root,
            Box::new(LinkPreviewCorePlugin::new()),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.linkpreview")?;

    // BL-133 — multi-channel notification dispatcher. Wires the
    // bus-event-based Desktop transport (shell renders the toast),
    // the Discord webhook transport, and the Telegram bot
    // transport. All three URLs / tokens / chat ids are sourced
    // from the forge config — empty when not configured, in which
    // case `send(channel: <c>, ...)` returns `NotConfigured` at
    // dispatch time rather than crashing at boot. SMTP, shell-side
    // settings UI, workflow-step, and agent-auto-notify wiring are
    // filed as follow-ups.
    let discord_webhook_url = load_discord_webhook_url(forge_root);
    let (telegram_bot_token, telegram_chat_id) = load_telegram_config(forge_root);
    let smtp_config = load_smtp_config(forge_root);
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.notifications",
                "Notifications",
                LifecycleFlags::NONE,
                &with_v1_aliases(&[(
                    "send",
                    nexus_notifications::core_plugin::HANDLER_SEND,
                )]),
            ),
            forge_root,
            Box::new(NotificationsCorePlugin::with_defaults(
                Some(Arc::clone(event_bus)),
                discord_webhook_url,
                telegram_bot_token,
                telegram_chat_id,
                smtp_config,
            )),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.notifications")?;

    // Audio — BL-117 STT + TTS subsystem. on_init loads the
    // `<forge>/.forge/config.toml::[audio]` block and builds the
    // configured backend pair (local / provider / platform). The
    // shipped build stubs `local` + `platform` so a forge without
    // an OPENAI_API_KEY surfaces a clear "backend not enabled"
    // error from the first dispatch rather than a panic.
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.audio",
                "Audio",
                LifecycleFlags {
                    on_init: true,
                    on_start: false,
                    on_stop: false,
                },
                &with_v1_aliases(&[
                    ("transcribe", nexus_audio::core_plugin::HANDLER_TRANSCRIBE),
                    ("synthesize", nexus_audio::core_plugin::HANDLER_SYNTHESIZE),
                    ("status", nexus_audio::core_plugin::HANDLER_STATUS),
                ]),
            ),
            forge_root,
            Box::new(AudioCorePlugin::new(forge_root.to_path_buf())),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.audio")?;

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

    // Agent system — PRD-15 scaffold. Thin dispatch surface over
    // `nexus-agent::{LlmAgent, PlanExecutor}`; bridges to `com.nexus.ai`
    // for planning and to arbitrary plugins for tool calls via the
    // `KernelPluginContext` wired below.
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.agent",
                "Agent",
                // BL-121 — on_init opens the transcript-search FTS
                // index. on_start / on_stop stay as no-ops.
                LifecycleFlags {
                    on_init: true,
                    on_start: false,
                    on_stop: false,
                },
                &with_v1_aliases(&[
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
                    // DG-32 (PRD-15 §4) — agent tool registry discovery.
                    ("list_tools", nexus_agent::HANDLER_LIST_TOOLS),
                    // DG-36 (PRD-15 §9) — custom .agent.toml manifests.
                    ("list_custom", nexus_agent::HANDLER_LIST_CUSTOM),
                    // DG-33 (PRD-15 §5) — agent-scoped persistent memory.
                    ("memory_record", nexus_agent::HANDLER_MEMORY_RECORD),
                    ("memory_query", nexus_agent::HANDLER_MEMORY_QUERY),
                    ("memory_prune", nexus_agent::HANDLER_MEMORY_PRUNE),
                    ("memory_export", nexus_agent::HANDLER_MEMORY_EXPORT),
                    // DG-37 (PRD-15 §10) — agent-to-agent delegation.
                    ("delegate", nexus_agent::HANDLER_DELEGATE),
                    // BL-121 — FTS5-backed transcript search over
                    // `.forge/agents/*/history.jsonl`.
                    (
                        "search_transcripts",
                        nexus_agent::core_plugin::HANDLER_SEARCH_TRANSCRIPTS,
                    ),
                ]),
            ),
            forge_root,
            Box::new(AgentCorePlugin::new_with_forge(forge_root.to_path_buf())),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.agent")?;

    // DG-32 — seed the agent-tool registry's process-global catalogue
    // once the agent core plugin is registered. Read by
    // `com.nexus.agent::list_tools` and by `nexus tool list`.
    nexus_agent::seed_default_tools();

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
                &with_v1_aliases(&[
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
                    (
                        "register_tool",
                        nexus_mcp::core_plugin::HANDLER_REGISTER_TOOL,
                    ),
                    (
                        "unregister_tool",
                        nexus_mcp::core_plugin::HANDLER_UNREGISTER_TOOL,
                    ),
                    (
                        "list_dynamic_tools",
                        nexus_mcp::core_plugin::HANDLER_LIST_DYNAMIC_TOOLS,
                    ),
                    // BL-113 Phase 3b — plugin contribution lifecycle.
                    (
                        "register_server",
                        nexus_mcp::core_plugin::HANDLER_REGISTER_SERVER,
                    ),
                    (
                        "unregister_server",
                        nexus_mcp::core_plugin::HANDLER_UNREGISTER_SERVER,
                    ),
                ]),
            ),
            forge_root,
            Box::new(McpHostPlugin::new(
                forge_root.to_path_buf(),
                Some(Arc::clone(event_bus)),
            )),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.mcp.host")?;

    // LSP host orchestrator — loads `<forge>/.forge/lsp.toml`, lazily spawns
    // configured language servers, and proxies LSP requests over IPC. Push
    // notifications (e.g. `publishDiagnostics`) fan out on the kernel bus
    // as `com.nexus.lsp.<method>`. BL-076.
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.lsp",
                "LSP Host",
                LifecycleFlags {
                    on_init: true,
                    on_start: true,
                    on_stop: true,
                },
                &with_v1_aliases(&[
                    ("list_servers", nexus_lsp::core_plugin::HANDLER_LIST_SERVERS),
                    ("open_file", nexus_lsp::core_plugin::HANDLER_OPEN_FILE),
                    ("close_file", nexus_lsp::core_plugin::HANDLER_CLOSE_FILE),
                    ("change_file", nexus_lsp::core_plugin::HANDLER_CHANGE_FILE),
                    ("completions", nexus_lsp::core_plugin::HANDLER_COMPLETIONS),
                    ("hover", nexus_lsp::core_plugin::HANDLER_HOVER),
                    ("definition", nexus_lsp::core_plugin::HANDLER_DEFINITION),
                    ("references", nexus_lsp::core_plugin::HANDLER_REFERENCES),
                    ("rename", nexus_lsp::core_plugin::HANDLER_RENAME),
                    ("code_actions", nexus_lsp::core_plugin::HANDLER_CODE_ACTIONS),
                    ("format", nexus_lsp::core_plugin::HANDLER_FORMAT),
                    (
                        "execute_command",
                        nexus_lsp::core_plugin::HANDLER_EXECUTE_COMMAND,
                    ),
                    // BL-113 Phase 2b — plugin contribution lifecycle.
                    (
                        "register_server",
                        nexus_lsp::core_plugin::HANDLER_REGISTER_SERVER,
                    ),
                    (
                        "unregister_server",
                        nexus_lsp::core_plugin::HANDLER_UNREGISTER_SERVER,
                    ),
                ]),
            ),
            forge_root,
            Box::new(LspCorePlugin::new(
                forge_root.to_path_buf(),
                Some(Arc::clone(event_bus)),
            )),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.lsp")?;

    // DAP host orchestrator — loads `<forge>/.forge/dap.toml`, lazily spawns
    // configured debug adapters, proxies DAP requests over IPC, and
    // republishes adapter-pushed events on the kernel bus as
    // `com.nexus.dap.<event>`. BL-081.
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.dap",
                "DAP Host",
                LifecycleFlags {
                    on_init: true,
                    on_start: true,
                    on_stop: true,
                },
                &with_v1_aliases(&[
                    ("list_adapters", nexus_dap::core_plugin::HANDLER_LIST_ADAPTERS),
                    ("launch", nexus_dap::core_plugin::HANDLER_LAUNCH),
                    ("attach", nexus_dap::core_plugin::HANDLER_ATTACH),
                    ("configuration_done", nexus_dap::core_plugin::HANDLER_CONFIGURATION_DONE),
                    ("disconnect", nexus_dap::core_plugin::HANDLER_DISCONNECT),
                    ("terminate", nexus_dap::core_plugin::HANDLER_TERMINATE),
                    ("set_breakpoints", nexus_dap::core_plugin::HANDLER_SET_BREAKPOINTS),
                    ("set_function_breakpoints", nexus_dap::core_plugin::HANDLER_SET_FUNCTION_BREAKPOINTS),
                    ("set_exception_breakpoints", nexus_dap::core_plugin::HANDLER_SET_EXCEPTION_BREAKPOINTS),
                    ("continue", nexus_dap::core_plugin::HANDLER_CONTINUE),
                    ("next", nexus_dap::core_plugin::HANDLER_NEXT),
                    ("step_in", nexus_dap::core_plugin::HANDLER_STEP_IN),
                    ("step_out", nexus_dap::core_plugin::HANDLER_STEP_OUT),
                    ("pause", nexus_dap::core_plugin::HANDLER_PAUSE),
                    ("threads", nexus_dap::core_plugin::HANDLER_THREADS),
                    ("stack_trace", nexus_dap::core_plugin::HANDLER_STACK_TRACE),
                    ("scopes", nexus_dap::core_plugin::HANDLER_SCOPES),
                    ("variables", nexus_dap::core_plugin::HANDLER_VARIABLES),
                    ("evaluate", nexus_dap::core_plugin::HANDLER_EVALUATE),
                    // BL-113 Phase 1c — plugin contribution lifecycle.
                    (
                        "register_adapter",
                        nexus_dap::core_plugin::HANDLER_REGISTER_ADAPTER,
                    ),
                    (
                        "unregister_adapter",
                        nexus_dap::core_plugin::HANDLER_UNREGISTER_ADAPTER,
                    ),
                ]),
            ),
            forge_root,
            Box::new(DapCorePlugin::new(
                forge_root.to_path_buf(),
                Some(Arc::clone(event_bus)),
            )),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.dap")?;

    // ACP host orchestrator — exposes the protocol-host contribution surface
    // for community-plugin-supplied agent adapters (BL-144 / ADR 0027 Phase 4).
    // No flat-TOML class — the registry starts empty and is populated at
    // plugin-load time by `acp_contribution_wiring::wire_acp_contributions`.
    // Agent-pushed notifications fan out on the kernel bus as
    // `com.nexus.acp.<method-with-dots>`.
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.acp",
                "ACP Host",
                LifecycleFlags {
                    on_init: true,
                    on_start: true,
                    on_stop: true,
                },
                &with_v1_aliases(&[
                    ("list_agents", nexus_acp::core_plugin::HANDLER_LIST_AGENTS),
                    ("initialize", nexus_acp::core_plugin::HANDLER_INITIALIZE),
                    ("propose", nexus_acp::core_plugin::HANDLER_PROPOSE),
                    ("accept", nexus_acp::core_plugin::HANDLER_ACCEPT),
                    ("reject", nexus_acp::core_plugin::HANDLER_REJECT),
                    ("disconnect", nexus_acp::core_plugin::HANDLER_DISCONNECT),
                    // BL-113 Phase 4 — plugin contribution lifecycle.
                    (
                        "register_server",
                        nexus_acp::core_plugin::HANDLER_REGISTER_SERVER,
                    ),
                    (
                        "unregister_server",
                        nexus_acp::core_plugin::HANDLER_UNREGISTER_SERVER,
                    ),
                ]),
            ),
            forge_root,
            Box::new(AcpCorePlugin::new(
                forge_root.to_path_buf(),
                Some(Arc::clone(event_bus)),
            )),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.acp")?;

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
                ]),
            ),
            forge_root,
            Box::new(GitCorePlugin::new(
                forge_root.to_path_buf(),
                Some(Arc::clone(event_bus)),
            )),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.git")?;

    // Terminal & process manager — PRD-09. Pure-library crate wrapped
    // behind `com.nexus.terminal` so UI / script plugins reach it over
    // dispatch rather than linking it directly (ARCHITECTURE §7 invariant #3).
    // Saved-commands (§14.1) and ad-hoc history (§10) share the same
    // SQLite file at `<forge>/.forge/procmgr.sqlite` — separate tables,
    // separate `Connection`s. Failure to open either store is logged
    // and the plugin loads without that handler family (session IPC
    // stays usable even when SQLite misbehaves).
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
    let terminal_plugin = match nexus_terminal::SqliteAdHocStore::open(&saved_db) {
        Ok(store) => terminal_plugin.with_adhoc_store(store),
        Err(err) => {
            tracing::warn!(
                path = %saved_db.display(),
                err = %err,
                "com.nexus.terminal: ad-hoc history store unavailable; adhoc_* handlers will return errors"
            );
            terminal_plugin
        }
    };
    // BL-061 — memory monitor: enabled by default with PRD-09 §7.3
    // recommended limits (250 MB soft / 500 MB hard). The poller is
    // spawned alongside the byte-stream drainer in `with_event_bus`;
    // the order here matters so `with_event_bus` sees the configured
    // monitor and starts the poller. BL-061 follow-up (2026-05-08)
    // wired per-saved-command overrides via `SavedCommand.memory_limit_mb`:
    // `dispatch_run_saved` stages the override so the next poller
    // round applies it instead of the bootstrap-wide default.
    // Operators that want measure-only semantics for ad-hoc sessions
    // (RSS chip but no auto-kill) still ride this default — saved
    // commands with an explicit limit are the per-session path.
    let terminal_plugin =
        terminal_plugin.with_memory_monitor(nexus_terminal::MemoryLimits::default_recommended());
    // BL-062 — install an eviction persister that durably stashes
    // the scrollback of any LRU-evicted session. BL-063 — share the
    // same `SqliteSessionStore` handle with the plugin so the
    // `cross_session_search` handler can read the FTS5 index that
    // the persister populates on every save. Without this hook the
    // snapshot is dropped silently and search returns "store not
    // configured" (matching pre-BL-062 behaviour). The session store
    // sits alongside the saved / adhoc stores at
    // `<forge>/.forge/sessions.sqlite`; scrollback blobs land at
    // `<forge>/.forge/sessions/<session_id>/scrollback.bin`.
    let session_db = forge_root.join(".forge").join("sessions.sqlite");
    let scrollback_dir = forge_root.join(".forge").join("sessions");
    let terminal_plugin = match nexus_terminal::SqliteSessionStore::open(
        &session_db,
        &scrollback_dir,
    ) {
        Ok(store) => {
            let store = Arc::new(std::sync::Mutex::new(store));
            let persister_store = Arc::clone(&store);
            let persister: nexus_terminal::EvictionPersister = Box::new(move |id, bytes| {
                let g = persister_store
                    .lock()
                    .map_err(|_| nexus_terminal::TerminalError::Persist(
                        "eviction persister: store mutex poisoned".into(),
                    ))?;
                g.save_scrollback(id, bytes)
            });
            terminal_plugin
                .with_eviction_persister(persister)
                .with_session_store(store)
        }
        Err(err) => {
            tracing::warn!(
                path = %session_db.display(),
                err = %err,
                "com.nexus.terminal: session store unavailable; LRU-evicted scrollback will be dropped",
            );
            terminal_plugin
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
                &with_v1_aliases(&[
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
                    (
                        "open_in_terminal",
                        nexus_terminal::HANDLER_OPEN_IN_TERMINAL,
                    ),
                    ("adhoc_list", nexus_terminal::HANDLER_ADHOC_LIST),
                    ("adhoc_get", nexus_terminal::HANDLER_ADHOC_GET),
                    ("adhoc_delete", nexus_terminal::HANDLER_ADHOC_DELETE),
                    ("adhoc_promote", nexus_terminal::HANDLER_ADHOC_PROMOTE),
                    ("run_saved", nexus_terminal::HANDLER_RUN_SAVED),
                    ("suggest", nexus_terminal::HANDLER_SUGGEST),
                    (
                        "cross_session_search",
                        nexus_terminal::HANDLER_CROSS_SESSION_SEARCH,
                    ),
                ]),
            ),
            forge_root,
            Box::new(terminal_plugin),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.terminal")?;

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
/// Capabilities granted to the `com.nexus.audio` plugin context
/// (BL-117). The provider-routed backend uses two kernel surfaces:
///
/// - `IpcCall` to reach `com.nexus.ai::resolve_credentials` for the
///   active chat provider's key.
/// - `NetHttp` for outbound HTTPS to the AI provider's audio
///   endpoint.
///
/// The audio plugin does NOT request `AudioRecord` / `AudioSynthesize`
/// — those are *caller-facing* gates declared on the handler manifest
/// (`add_cap_requirement` above), enforced when something *outside*
/// the audio plugin calls `com.nexus.audio::transcribe` /
/// `synthesize`. The audio plugin's own context never needs to call
/// those handlers on itself.
#[must_use]
pub fn audio_capabilities() -> CapabilitySet {
    [Capability::IpcCall, Capability::NetHttp]
        .into_iter()
        .collect()
}

/// Capabilities granted to the `com.nexus.workflow` `KernelPluginContext`
/// at runtime wiring time (issue #73). Scoped to `IpcCall` + `AiChat`
/// only — every step type (ipc_call / ai_prompt / digest reads / …)
/// routes through `ctx.ipc_call(...)`, gated by the target plugin's
/// own capability checks.
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

/// BL-133 — pull `[notifications.discord].webhook_url` out of
/// `<forge>/.forge/config.toml`. Missing file / section / field all
/// fall back to an empty string, which surfaces at
/// `send(channel: discord, ...)` time as
/// `SendError::NotConfigured` rather than crashing at boot.
fn load_discord_webhook_url(forge_root: &std::path::Path) -> String {
    #[derive(serde::Deserialize, Default)]
    struct Wrapper {
        #[serde(default)]
        notifications: Notifications,
    }
    #[derive(serde::Deserialize, Default)]
    struct Notifications {
        #[serde(default)]
        discord: DiscordCfg,
    }
    #[derive(serde::Deserialize, Default)]
    struct DiscordCfg {
        #[serde(default)]
        webhook_url: String,
    }
    let path = forge_root.join(".forge").join("config.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return String::new(),
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                %err,
                "config.toml: read failed; discord webhook url unset"
            );
            return String::new();
        }
    };
    match toml::from_str::<Wrapper>(&text) {
        Ok(w) => w.notifications.discord.webhook_url,
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                %err,
                "config.toml: [notifications.discord] failed to parse; webhook url unset"
            );
            String::new()
        }
    }
}

/// BL-133 follow-up — pull `[notifications.telegram].bot_token` +
/// `chat_id` out of `<forge>/.forge/config.toml`. Returns
/// `(bot_token, chat_id)`; either field missing falls back to an
/// empty string, surfacing at send-time as
/// `SendError::NotConfigured`. Same fallback behaviour as
/// [`load_discord_webhook_url`]. Future-compat: when the keyring
/// path lands, swap the inner reads for `nexus-security` IPC calls
/// while keeping the same return shape.
fn load_telegram_config(forge_root: &std::path::Path) -> (String, String) {
    #[derive(serde::Deserialize, Default)]
    struct Wrapper {
        #[serde(default)]
        notifications: Notifications,
    }
    #[derive(serde::Deserialize, Default)]
    struct Notifications {
        #[serde(default)]
        telegram: TelegramCfg,
    }
    #[derive(serde::Deserialize, Default)]
    struct TelegramCfg {
        #[serde(default)]
        bot_token: String,
        #[serde(default)]
        chat_id: String,
    }
    let path = forge_root.join(".forge").join("config.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return (String::new(), String::new());
        }
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                %err,
                "config.toml: read failed; telegram credentials unset"
            );
            return (String::new(), String::new());
        }
    };
    match toml::from_str::<Wrapper>(&text) {
        Ok(w) => (w.notifications.telegram.bot_token, w.notifications.telegram.chat_id),
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                %err,
                "config.toml: [notifications.telegram] failed to parse; credentials unset"
            );
            (String::new(), String::new())
        }
    }
}

/// BL-133 follow-up — pull `[notifications.email]` out of
/// `<forge>/.forge/config.toml`. Missing file / section / fields fall
/// back to a `SmtpConfig::default()` shape (every field empty / port 0)
/// which surfaces at `send(channel: email, ...)` time as
/// `SendError::NotConfigured`. Same fallback behaviour as
/// [`load_telegram_config`]. The plain-text password in config.toml is
/// a deliberate v1 trade-off; the BL-133 follow-up tail tracks moving
/// the credential through the `nexus-security` keyring once the IPC
/// surface lands.
fn load_smtp_config(forge_root: &std::path::Path) -> nexus_notifications::SmtpConfig {
    #[derive(serde::Deserialize, Default)]
    struct Wrapper {
        #[serde(default)]
        notifications: Notifications,
    }
    #[derive(serde::Deserialize, Default)]
    struct Notifications {
        #[serde(default)]
        email: EmailCfg,
    }
    #[derive(serde::Deserialize, Default)]
    struct EmailCfg {
        #[serde(default)]
        host: String,
        #[serde(default)]
        port: u16,
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: String,
        #[serde(default)]
        from: String,
        #[serde(default)]
        to: String,
        #[serde(default)]
        subject_template: String,
    }
    let path = forge_root.join(".forge").join("config.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return nexus_notifications::SmtpConfig::default();
        }
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                %err,
                "config.toml: read failed; smtp credentials unset"
            );
            return nexus_notifications::SmtpConfig::default();
        }
    };
    match toml::from_str::<Wrapper>(&text) {
        Ok(w) => nexus_notifications::SmtpConfig {
            host: w.notifications.email.host,
            port: w.notifications.email.port,
            username: w.notifications.email.username,
            password: w.notifications.email.password,
            from: w.notifications.email.from,
            to: w.notifications.email.to,
            subject_template: w.notifications.email.subject_template,
        },
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                %err,
                "config.toml: [notifications.email] failed to parse; smtp credentials unset"
            );
            nexus_notifications::SmtpConfig::default()
        }
    }
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

#[cfg(test)]
mod or_lifecycle_skip_tests {
    use std::sync::Arc;

    use nexus_kernel::{EventBus, EventFilter, NexusEvent};
    use nexus_plugins::PluginError;

    use super::RegisterCoreResultExt;

    /// BL-095 follow-up — a `LifecycleTimeout` error is converted to
    /// `Ok(())` and a `com.nexus.kernel.plugin_lifecycle_timeout`
    /// event lands on the bus carrying the plugin id, the hook name,
    /// and the timeout value. The shell can subscribe to this and
    /// surface a "<plugin> failed to start" notice.
    #[test]
    fn lifecycle_timeout_skips_and_publishes_bus_event() {
        let bus = Arc::new(EventBus::new(16));
        let mut sub = bus.subscribe(EventFilter::CustomPrefix(
            "com.nexus.kernel.".to_string(),
        ));
        let result: Result<nexus_plugins::PluginInfo, PluginError> =
            Err(PluginError::LifecycleTimeout {
                plugin_id: "com.nexus.test".to_string(),
                hook: "init".to_string(),
                timeout_secs: 30,
            });
        let outcome = result.or_lifecycle_skip(&bus, "com.nexus.test");
        assert!(outcome.is_ok(), "lifecycle timeout should be swallowed, got {outcome:?}");
        let ev = sub
            .try_recv()
            .expect("bus alive")
            .expect("expected one published event");
        match &ev.event {
            NexusEvent::Custom { type_id, payload, .. } => {
                assert_eq!(type_id, "com.nexus.kernel.plugin_lifecycle_timeout");
                assert_eq!(payload["plugin_id"], "com.nexus.test");
                assert_eq!(payload["hook"], "\"init\"");
                assert_eq!(payload["timeout_secs"], 30);
            }
            other => panic!("expected Custom event, got {other:?}"),
        }
    }

    /// Non-timeout errors still abort with the original anyhow
    /// context attached. The skip path is narrow on purpose: a
    /// manifest-invalid or duplicate-id error is a programming bug,
    /// not a "slow plugin" we should silently skip past.
    #[test]
    fn non_timeout_errors_still_propagate() {
        let bus = Arc::new(EventBus::new(16));
        let result: Result<nexus_plugins::PluginInfo, PluginError> =
            Err(PluginError::DuplicatePlugin("com.nexus.test".to_string()));
        let outcome = result.or_lifecycle_skip(&bus, "com.nexus.test");
        let err = outcome.expect_err("duplicate-id should propagate");
        let msg = err.to_string();
        assert!(
            msg.contains("failed to register com.nexus.test"),
            "context label missing in {msg}",
        );
    }

    // The non-error path is exercised live by every other test in
    // the bootstrap suite (every successful boot routes through
    // `or_lifecycle_skip`); a synthetic `Ok(PluginInfo)` test would
    // have to fabricate a real `PluginInfo` whose constructor isn't
    // public, so we skip it here.
}
