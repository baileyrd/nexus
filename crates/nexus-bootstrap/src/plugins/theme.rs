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

/// C87 — `~/.nexus/themes` and `~/.nexus/snippets`: the directories the
/// shipped Theme Builder already instructs users to save into
/// (`shell/src/plugins/nexus/themePicker/ThemeBuilder.tsx`) and
/// `nexus-theme`'s discovery/watcher already know how to scan — bootstrap
/// just never pointed a [`ThemeCorePlugin`] at them. Falls back to a
/// `.nexus/<leaf>` relative path (matching `nexus-plugins::signing`'s
/// `~/.nexus/keys` resolver) when the home directory can't be resolved,
/// so this never panics on an unusual environment.
fn user_nexus_dir(leaf: &str) -> std::path::PathBuf {
    dirs::home_dir().map_or_else(
        || std::path::PathBuf::from(".nexus").join(leaf),
        |h| h.join(".nexus").join(leaf),
    )
}

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    let themes_dir = user_nexus_dir("themes");
    let snippets_dir = user_nexus_dir("snippets");
    loader
        .register_core(
            core_manifest_with_ipc(
                "com.nexus.theme",
                "Theme",
                LifecycleFlags {
                    on_init: false,
                    // C87 — on_start/on_stop launch and tear down the
                    // ThemeWatcher background thread (see
                    // `ThemeCorePlugin::on_start`/`on_stop`).
                    on_start: true,
                    on_stop: true,
                },
                &with_v1_aliases(nexus_theme::core_plugin::IPC_HANDLERS),
            ),
            forge_root,
            Box::new(ThemeCorePlugin::with_dirs(
                themes_dir,
                snippets_dir,
                Some(Arc::clone(event_bus)),
            )),
        )
        // Non-essential UX feature: a lifecycle hang (or the watcher
        // failing to start, which `on_start` already soft-fails
        // internally) must never take down boot. `or_lifecycle_skip`
        // degrades to "theme plugin unavailable" rather than aborting.
        .or_lifecycle_skip(event_bus, "com.nexus.theme")?;
    Ok(())
}
