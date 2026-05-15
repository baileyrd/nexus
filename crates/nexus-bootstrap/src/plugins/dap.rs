//! DAP Host plugin registration.
//!
//! Loads `<forge>/.forge/dap.toml`, lazily spawns configured debug
//! adapters, proxies DAP requests over IPC, and republishes adapter-
//! pushed events on the kernel bus as `com.nexus.dap.<event>`. BL-081.

use std::sync::Arc;

use anyhow::Result;
use nexus_dap::DapCorePlugin;
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
    Ok(())
}
