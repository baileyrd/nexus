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
use nexus_plugins::{CorePlugin, PluginError, PluginLoader, PluginManifest, SharedPluginLoader};

pub mod agent;
mod audit_sqlite;
/// BL-138 — TOML-driven per-handler capability matrix loader. See
/// [`cap_matrix::apply`] and the companion `cap_matrix.toml`.
pub mod cap_matrix;
/// BL-138 — named args-aware capability policies referenceable from
/// the cap matrix. See [`cap_policies::resolve`].
pub mod cap_policies;
pub mod collab;
pub mod crdt_publisher;
pub mod database;
pub mod dream_cycle;
pub mod forge_template;
/// BL-140 Phase 2b — `IpcInvoker` trait abstracting local vs remote
/// IPC dispatch. The CLI consumes this trait so the same subcommand
/// body works against both shapes.
pub mod invoker;
/// AI-first-class capture: pumps every kernel event into the native memory store.
pub mod memory_capture;
mod plugins;
/// BL-113 / ADR 0027 — manifest-side `ContributedAdapter` → host-side
/// `{Lsp,Mcp}ServerSpec` converters. Phase 2a/3a primitive; the
/// bootstrap-side wiring that calls these converters and feeds the
/// result through `merge_contributed` lands in Phase 2b/3b once the
/// plugin-lifecycle callback shape is settled by Phase 1.
pub mod protocol_host_specs;
/// BL-140 Phase 2c — reconnect-on-drop wrapper layered over
/// [`remote::RemoteRuntime`].
pub mod reconnect;
/// BL-140 Phase 2b — `RemoteRuntime` factory + SSH child-process
/// transport for `--forge-path ssh://...` URIs.
pub mod remote;
pub mod storage;
pub mod terminal;

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

/// Parse a `.canvas` JSON string.
pub use nexus_formats::canvas::parse as parse_canvas;
/// Serialize a [`CanvasFile`] to pretty-printed JSON.
pub use nexus_formats::canvas::serialize as serialize_canvas;
/// Canvas data types and pure (de)serialization helpers, re-exported from
/// `nexus-formats` so CLI/TUI canvas commands can parse and mutate canvas
/// files without pulling in the SQLite-backed `nexus-storage` crate.
pub use nexus_formats::{CanvasEdge, CanvasEdgeType, CanvasFile, CanvasNode, CanvasNodeType};

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
    ///
    /// #201 / R18 — `#[doc(hidden)]` so rustdoc / IDE completion
    /// don't surface the loader as a casual handle. The field stays
    /// `pub` because shell/bootstrap test consumers in different
    /// crates need direct field access; the marker just discourages
    /// new consumers from reaching for it when `context` would do.
    #[doc(hidden)]
    pub loader: Arc<SharedPluginLoader>,
}

impl Runtime {
    /// Return an [`IpcInvoker`](invoker::IpcInvoker) trait object
    /// backed by this runtime's [`KernelPluginContext`]. BL-140
    /// Phase 2b — exposed so CLI subcommands can be transport-agnostic
    /// (local kernel vs. remote SSH proxy).
    ///
    /// The returned `Arc` clones the underlying context internally;
    /// the original `Runtime` keeps full access to `context`,
    /// `kernel`, and `loader`.
    #[must_use]
    pub fn invoker(&self) -> Arc<dyn invoker::IpcInvoker + Send + Sync> {
        Arc::new(invoker::LocalIpcInvoker::new(self.context.clone()))
    }
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

fn build(
    forge_root: &std::path::Path,
    invoker_id: &'static str,
    invoker_name: &str,
) -> Result<Runtime> {
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
    // P1-09 — clamp every per-plugin WasmConfig against the kernel-wide
    // ceiling so a hostile manifest can't out-run metering.
    loader.set_wasm_caps_ceiling(Some(kernel.config().wasm_caps));

    // Register every in-tree core plugin. Order matters where lifecycle hooks
    // of later plugins rely on earlier ones publishing events; in practice
    // each plugin is independent today.
    plugins::register_all(&mut loader, forge_root, &event_bus)?;

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

    // BL-138 — apply the TOML-driven per-handler capability matrix.
    // The companion `cap_matrix.toml` is the single source of truth
    // for every handler that needs more than the default `ipc.call`
    // check, replacing the hand-maintained `add_cap_requirement(…)`
    // wall that previously lived here. Adds run before any plugin
    // dispatches, so the matrix is in effect by the time the first
    // `ipc_call` arrives.
    //
    // Issue #77 + ADR 0022 + ADR 0028 + BL-117 + BL-136 — every
    // historical entry from those landings is now a row in the
    // matrix file. See `cap_matrix.toml` for the rationale per
    // handler.
    cap_matrix::apply(&shared).context("failed to apply cap matrix")?;

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
    .context("failed to build kernel plugin context for com.nexus.ai")?
    .with_trust_level(nexus_kernel::TrustLevel::Core);
    shared
        .wire_context("com.nexus.ai", Arc::new(ai_ctx))
        .map_err(|e| anyhow::anyhow!("failed to wire AI plugin context: {e}"))?;

    // Memory plugin needs its own context so the async `recall` / `vector_sync`
    // handlers can `ipc_call` `com.nexus.ai::embed_text` (embed query/content)
    // and `com.nexus.storage`'s namespaced vector store. Core trust + full caps
    // mirror the AI plugin: the nested `embed_text` call is itself `ai.chat`-
    // gated, so the memory context must hold that cap to reach it.
    let memory_ctx = KernelPluginContext::new(
        "com.nexus.memory",
        env!("CARGO_PKG_VERSION"),
        all_caps(),
        Arc::clone(&kv_store),
        Arc::clone(&event_bus),
        forge_root,
        Some(Arc::clone(&dispatcher)),
    )
    .context("failed to build kernel plugin context for com.nexus.memory")?
    .with_trust_level(nexus_kernel::TrustLevel::Core);
    shared
        .wire_context("com.nexus.memory", Arc::new(memory_ctx))
        .map_err(|e| anyhow::anyhow!("failed to wire memory plugin context: {e}"))?;

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
    .context("failed to build kernel plugin context for com.nexus.agent")?
    .with_trust_level(nexus_kernel::TrustLevel::Core);
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
    .context("failed to build kernel plugin context for com.nexus.editor")?
    .with_trust_level(nexus_kernel::TrustLevel::Core);
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
    .context("failed to build kernel plugin context for com.nexus.workflow")?
    .with_trust_level(nexus_kernel::TrustLevel::Core);
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
    .context("failed to build kernel plugin context for com.nexus.audio")?
    .with_trust_level(nexus_kernel::TrustLevel::Core);
    shared
        .wire_context("com.nexus.audio", Arc::new(audio_ctx))
        .map_err(|e| anyhow::anyhow!("failed to wire audio plugin context: {e}"))?;

    // BL-134 Phase 1 — ai-runtime needs its own context so the
    // worker pool can `ctx.ipc_call("com.nexus.agent","session_run",
    // ..)` and `ctx.publish(...)` typed `AiEvent`s onto the bus.
    // Caps mirror `agent_capabilities()` (which is the underlying
    // call shape) plus the ai-runtime caps so the runtime can
    // recursively submit child tasks under Phase-2 delegate.
    let ai_runtime_ctx = KernelPluginContext::new(
        nexus_ai_runtime::PLUGIN_ID,
        env!("CARGO_PKG_VERSION"),
        ai_runtime_capabilities(),
        Arc::clone(&kv_store),
        Arc::clone(&event_bus),
        forge_root,
        Some(Arc::clone(&dispatcher)),
    )
    .context("failed to build kernel plugin context for com.nexus.ai.runtime")?
    .with_trust_level(nexus_kernel::TrustLevel::Core);
    shared
        .wire_context(nexus_ai_runtime::PLUGIN_ID, Arc::new(ai_runtime_ctx))
        .map_err(|e| anyhow::anyhow!("failed to wire ai-runtime plugin context: {e}"))?;

    // BL-143 Phase 1.2 — opt-in WebSocket relay bridge. Reads
    // `[collab]` from `.forge/config.toml`; spawns a `CollabClient`
    // bridging `com.nexus.editor.ops.*` events to a remote relay when
    // an ambient tokio runtime is reachable. Dropping the handle
    // detaches the task — its lifetime is tied to the tokio runtime
    // (i.e. the process), which is the right shape until BL-143 Phase
    // 1.5 wires explicit reconnect / shutdown.
    let _ = collab::start_if_enabled(forge_root, Arc::clone(&event_bus));

    // AI-first-class capture: feed every bus event into the native memory store
    // (loop-guarded + secret-redacted by nexus_memory::event_to_memory). Detached
    // like the collab relay; best-effort, never fatal to boot.
    let _ = memory_capture::start_capture(forge_root, Arc::clone(&event_bus));

    let context = KernelPluginContext::new(
        invoker_id,
        env!("CARGO_PKG_VERSION"),
        all_caps(),
        kv_store,
        event_bus,
        forge_root,
        Some(dispatcher),
    )
    .context("failed to build kernel plugin context for invoker")?
    .with_trust_level(nexus_kernel::TrustLevel::Core);

    Ok(Runtime {
        kernel,
        context,
        loader: shared,
    })
}

fn invoker_manifest(id: &str, name: &str) -> PluginManifest {
    plugins::core_manifest(id, name, plugins::LifecycleFlags::NONE)
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
        // BL-134 Phase 2b: `delegate` no longer runs `session_run`
        // inline — it submits an `AgentTaskKind::Session` to
        // `com.nexus.ai.runtime` and awaits via `wait_for`. The two
        // caps below gate those calls. No `ai.runtime.control` —
        // delegate never cancels / pauses / resumes the sub-task; if
        // a parent session aborts, the child runs to completion
        // observably and the parent surfaces the timeout.
        Capability::AiRuntimeSubmit,
        Capability::AiRuntimeObserve,
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

/// Capabilities granted to the `com.nexus.ai.runtime` plugin context
/// (BL-134 Phase 1). The runtime worker pool dispatches into the
/// agent (`session_run`) and republishes typed events on the kernel
/// bus.
///
/// - `IpcCall` to reach `com.nexus.agent::session_run` (and Phase 2+
///   `com.nexus.ai::stream_chat` for `AgentTaskKind::AiStream`).
/// - `AiChat` because `session_run` is gated on it (per ADR 0022 /
///   ADR 0024). The runtime impersonates the caller's caps when it
///   issues these IPC calls; per-call capability snapshotting lands
///   in Phase 2 alongside `delegate`.
/// - `EventsPublish` so `record_and_publish` can emit
///   `com.nexus.ai.runtime.*` topics.
#[must_use]
pub fn ai_runtime_capabilities() -> CapabilitySet {
    [
        Capability::IpcCall,
        Capability::AiChat,
        Capability::EventsPublish,
    ]
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
    //
    // BL-134 Phase 3: `async = true` steps route through
    // `com.nexus.ai.runtime::submit` so they don't block the
    // workflow's per-step await loop. `ai.runtime.observe` is NOT
    // granted — the workflow records the returned `task_id` and a
    // follow-up step (sync) can `wait_for` it if needed; the
    // observe gate is checked at *that* step rather than at submit
    // time.
    [
        Capability::IpcCall,
        Capability::AiChat,
        Capability::AiRuntimeSubmit,
    ]
    .into_iter()
    .collect()
}

/// BL-133 — pull `[notifications.discord].webhook_url` out of
/// `<forge>/.forge/config.toml`. Missing file / section / field all
/// fall back to an empty string, which surfaces at
/// `send(channel: discord, ...)` time as
/// `SendError::NotConfigured` rather than crashing at boot.
/// BL-135 — load the unified `notifications.toml` router config.
///
/// Returns `(config, Some(path))` when the file exists (path drives
/// the live-reload watcher inside the notifications plugin).
/// Returns `(synthetic_config, None)` when the file is absent — the
/// synthetic config carries the legacy `config.toml::[notifications.*]`
/// channel credentials so a pre-BL-135 forge keeps delivering on the
/// override path (CLI `nexus notify send --channel discord …`,
/// workflow `notify` steps with explicit `channel = "…"`). Source-
/// tagged sends in that state route nowhere — authors opting in to
/// BL-135 routing add a `notifications.toml`.
fn load_notifications_config(
    forge_root: &std::path::Path,
) -> (
    nexus_notifications::NotificationsConfig,
    Option<std::path::PathBuf>,
) {
    let path = forge_root.join(nexus_notifications::NOTIFICATIONS_CONFIG_RELPATH);
    if path.exists() {
        match nexus_notifications::NotificationsConfig::load_from(&path) {
            Ok(cfg) => return (cfg, Some(path)),
            Err(err) => tracing::warn!(
                path = %path.display(),
                %err,
                "notifications.toml: parse failed; falling back to legacy [notifications.*] blocks"
            ),
        }
    }
    let mut cfg = nexus_notifications::NotificationsConfig::default();
    cfg.channels.discord.webhook_url = load_discord_webhook_url(forge_root);
    let (bot_token, chat_id) = load_telegram_config(forge_root);
    cfg.channels.telegram.bot_token = bot_token;
    cfg.channels.telegram.chat_id = chat_id;
    let smtp = load_smtp_config(forge_root);
    cfg.channels.email.host = smtp.host;
    cfg.channels.email.port = smtp.port;
    cfg.channels.email.username = smtp.username;
    cfg.channels.email.password = smtp.password;
    cfg.channels.email.from = smtp.from;
    cfg.channels.email.to = smtp.to;
    cfg.channels.email.subject_template = smtp.subject_template;
    // Synthesize default source routes so source-tagged producers
    // keep delivering on Desktop when the forge hasn't authored a
    // `notifications.toml`. Users opt into per-source routing
    // (Discord/Telegram/email) by creating the file.
    for source in ["workflow", "agent", "cli", "ai_runtime"] {
        cfg.sources.insert(
            source.to_string(),
            nexus_notifications::SourceConfig {
                route: vec!["desktop".to_string()],
                ..Default::default()
            },
        );
    }
    (cfg, None)
}

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
        Ok(w) => (
            w.notifications.telegram.bot_token,
            w.notifications.telegram.chat_id,
        ),
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
