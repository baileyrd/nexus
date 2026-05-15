//! Theme plugin registration.
//!
//! Theme engine — registered as a core plugin so (a) other plugins can
//! call `ipc_call("com.nexus.theme", …)` and subscribe to
//! `com.nexus.theme.changed` events, and (b) the Tauri shell's theme
//! commands are thin adapters over kernel IPC rather than owning
//! engine state directly. See PRD-07.

use std::sync::Arc;

use anyhow::Result;
use nexus_kernel::EventBus;
use nexus_plugins::PluginLoader;
use nexus_theme::ThemeCorePlugin;

use super::{core_manifest_with_ipc, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
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
    Ok(())
}
