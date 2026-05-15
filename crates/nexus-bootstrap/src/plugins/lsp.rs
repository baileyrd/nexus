//! LSP host plugin registration.
//!
//! Loads `<forge>/.forge/lsp.toml`, lazily spawns configured language
//! servers, and proxies LSP requests over IPC. Push notifications
//! (e.g. `publishDiagnostics`) fan out on the kernel bus as
//! `com.nexus.lsp.<method>`. BL-076.

use std::sync::Arc;

use anyhow::Result;
use nexus_kernel::EventBus;
use nexus_lsp::LspCorePlugin;
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
    Ok(())
}
