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
    let kernel = Kernel::new(config).context("failed to build kernel")?;
    let event_bus: Arc<EventBus> = kernel.event_bus();
    let kv_store = kernel.kv_store();

    let plugins_dir = forge_root.join(".forge").join("plugins");
    let mut loader = PluginLoader::new(&plugins_dir);
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
    use nexus_ai::AiCorePlugin;
    use nexus_database::DatabaseCorePlugin;
    use nexus_git::GitCorePlugin;
    use nexus_security::SecurityCorePlugin;
    use nexus_storage::{StorageConfig, StorageCorePlugin};

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
                    ("query_files", nexus_storage::core_plugin::HANDLER_QUERY_FILES),
                    ("read_file", nexus_storage::core_plugin::HANDLER_READ_FILE),
                    ("backlinks", nexus_storage::core_plugin::HANDLER_BACKLINKS),
                    ("query_tasks", nexus_storage::core_plugin::HANDLER_QUERY_TASKS),
                    ("graph_stats", nexus_storage::core_plugin::HANDLER_GRAPH_STATS),
                    ("rebuild_index", nexus_storage::core_plugin::HANDLER_REBUILD_INDEX),
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

    loader
        .register_core(
            core_manifest(
                "com.nexus.git",
                "Git",
                LifecycleFlags {
                    on_init: true,
                    ..LifecycleFlags::NONE
                },
            ),
            forge_root,
            Box::new(GitCorePlugin::new(forge_root.to_path_buf())),
        )
        .context("failed to register com.nexus.git")?;

    loader
        .register_core(
            core_manifest(
                "com.nexus.database",
                "Database",
                LifecycleFlags {
                    on_init: true,
                    on_start: true,
                    on_stop: true,
                },
            ),
            forge_root,
            Box::new(DatabaseCorePlugin::new(Arc::clone(event_bus))),
        )
        .context("failed to register com.nexus.database")?;

    loader
        .register_core(
            core_manifest(
                "com.nexus.ai",
                "AI",
                LifecycleFlags {
                    on_init: true,
                    ..LifecycleFlags::NONE
                },
            ),
            forge_root,
            Box::new(AiCorePlugin::new()),
        )
        .context("failed to register com.nexus.ai")?;

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
