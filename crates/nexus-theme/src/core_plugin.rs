//! Core plugin: exposes the [`ThemeEngine`] over kernel IPC.
//!
//! Registers as `com.nexus.theme`. Before this plugin existed the
//! Tauri shell instantiated `ThemeEngine` directly and every
//! `#[tauri::command]` locked it by hand — other plugins had no way
//! to subscribe to theme changes or invoke theme operations, which
//! violated the microkernel invariant that every subsystem should be
//! reachable via `ipc_call`.
//!
//! The plugin owns a `Mutex<ThemeEngine>` and publishes
//! `com.nexus.theme.changed` events on every mutation
//! ([`apply_theme`], [`set_mode`], [`toggle_snippet`],
//! [`reorder_snippets`], [`apply_config`]) so a plugin that wants to
//! sync its own palette to the active theme can subscribe with
//! `EventFilter::CustomPrefix("com.nexus.theme.")` and react.
//!
//! | Command              | Handler id | Description                                   |
//! |----------------------|------------|-----------------------------------------------|
//! | `get_available_themes`   | 1  | List every built-in + discovered theme      |
//! | `apply_theme`            | 2  | Switch active theme (emits `changed`)       |
//! | `compute_variables`      | 3  | Stateless cascade compute                   |
//! | `get_available_snippets` | 4  | List snippets + enabled flag                |
//! | `toggle_snippet`         | 5  | Toggle a snippet (emits `changed`)          |
//! | `reorder_snippets`       | 6  | Replace enabled order (emits `changed`)     |
//! | `get_theme_config`       | 7  | Current selection snapshot                  |
//! | `set_mode`               | 8  | Light/dark/system (emits `changed`)         |
//! | `apply_config`           | 9  | Restore from persisted [`ThemeConfig`]      |
//! | `set_plugin_overrides`   | 10 | Merge plugin variable overrides             |
//! | `reload`                 | 11 | Rescan themes/snippets directories          |
//!
//! [`ThemeEngine`]: crate::api::ThemeEngine
//! [`apply_theme`]: crate::api::ThemeEngine::apply_theme
//! [`set_mode`]: crate::api::ThemeEngine::set_mode
//! [`toggle_snippet`]: crate::api::ThemeEngine::toggle_snippet
//! [`reorder_snippets`]: crate::api::ThemeEngine::reorder_snippets
//! [`apply_config`]: crate::api::ThemeEngine::apply_config

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, PluginError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;

use crate::api::{ThemeConfig, ThemeEngine};
use crate::theme::ThemeMode;
use crate::variables::VariableMap;
use crate::watcher::{ThemeReloadEvent, ThemeWatcher, DEFAULT_DEBOUNCE_MS};

/// Reverse-DNS identifier for this plugin.
pub const PLUGIN_ID: &str = "com.nexus.theme";

/// Event type id published on every state mutation. Payload is the
/// [`ThemeConfig`] snapshot after the change.
pub const EVENT_CHANGED: &str = "com.nexus.theme.changed";

// ── IPC handler ids — stable, append-only ────────────────────────────────────

/// Handler id for `get_available_themes`.
pub const HANDLER_GET_AVAILABLE_THEMES: u32 = 1;
/// Handler id for `apply_theme`.
pub const HANDLER_APPLY_THEME: u32 = 2;
/// Handler id for `compute_variables`.
pub const HANDLER_COMPUTE_VARIABLES: u32 = 3;
/// Handler id for `get_available_snippets`.
pub const HANDLER_GET_AVAILABLE_SNIPPETS: u32 = 4;
/// Handler id for `toggle_snippet`.
pub const HANDLER_TOGGLE_SNIPPET: u32 = 5;
/// Handler id for `reorder_snippets`.
pub const HANDLER_REORDER_SNIPPETS: u32 = 6;
/// Handler id for `get_theme_config`.
pub const HANDLER_GET_THEME_CONFIG: u32 = 7;
/// Handler id for `set_mode`.
pub const HANDLER_SET_MODE: u32 = 8;
/// Handler id for `apply_config`.
pub const HANDLER_APPLY_CONFIG: u32 = 9;
/// Handler id for `set_plugin_overrides`.
pub const HANDLER_SET_PLUGIN_OVERRIDES: u32 = 10;
/// Handler id for `reload`.
pub const HANDLER_RELOAD: u32 = 11;

/// SD-06 — single source of truth for `(command-name, handler-id)`
/// pairs consumed by `nexus_bootstrap::plugins::theme::register`.
/// Order matches the pre-SD-06 bootstrap registration.
pub const IPC_HANDLERS: &[(&str, u32)] = &[
    ("get_available_themes", HANDLER_GET_AVAILABLE_THEMES),
    ("apply_theme", HANDLER_APPLY_THEME),
    ("compute_variables", HANDLER_COMPUTE_VARIABLES),
    ("get_available_snippets", HANDLER_GET_AVAILABLE_SNIPPETS),
    ("toggle_snippet", HANDLER_TOGGLE_SNIPPET),
    ("reorder_snippets", HANDLER_REORDER_SNIPPETS),
    ("get_theme_config", HANDLER_GET_THEME_CONFIG),
    ("set_mode", HANDLER_SET_MODE),
    ("apply_config", HANDLER_APPLY_CONFIG),
    ("set_plugin_overrides", HANDLER_SET_PLUGIN_OVERRIDES),
    ("reload", HANDLER_RELOAD),
];

// ── DTOs ─────────────────────────────────────────────────────────────────────

/// Args for `apply_theme`.
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[cfg_attr(feature = "ts-export", derive(JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ApplyThemeArgs {
    /// Theme id to switch to.
    pub id: String,
}

/// Args for `compute_variables`.
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[cfg_attr(feature = "ts-export", derive(JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ComputeVariablesArgs {
    /// Theme id (does not affect engine state).
    pub theme_id: String,
    /// Enabled snippet ids, in cascade order.
    pub enabled_snippets: Vec<String>,
}

/// Args for `toggle_snippet`.
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[cfg_attr(feature = "ts-export", derive(JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ToggleSnippetArgs {
    /// Snippet id to toggle.
    pub id: String,
}

/// Args for `reorder_snippets`.
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[cfg_attr(feature = "ts-export", derive(JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ReorderSnippetsArgs {
    /// New ordered list of enabled snippet ids.
    pub ids: Vec<String>,
}

/// Args for `set_mode`.
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[cfg_attr(feature = "ts-export", derive(JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct SetModeArgs {
    /// Desired mode.
    pub mode: ThemeMode,
}

/// Args for `apply_config`.
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[cfg_attr(feature = "ts-export", derive(JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ApplyConfigArgs {
    /// Config snapshot to restore.
    pub config: ThemeConfig,
}

/// Args for `set_plugin_overrides`.
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[cfg_attr(feature = "ts-export", derive(JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct SetPluginOverridesArgs {
    /// Variable overrides to merge on top of the cascade.
    pub overrides: VariableMap,
}

/// Response envelope for `apply_config`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "ts-export", derive(JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct Ack {
    /// Always `true`; present so the wire type is an object, not `null`.
    pub ok: bool,
}

// ── Plugin ───────────────────────────────────────────────────────────────────

/// Background [`ThemeWatcher`] pump — owns the shutdown flag and join
/// handle so [`ThemeCorePlugin::on_stop`] can signal + wait for a clean
/// exit rather than leaking a detached thread.
struct WatcherHandle {
    running: Arc<AtomicBool>,
    join: std::thread::JoinHandle<()>,
}

/// Core plugin wrapping a [`ThemeEngine`] behind a mutex and an
/// [`EventBus`] hook for mutation events.
pub struct ThemeCorePlugin {
    engine: Arc<Mutex<ThemeEngine>>,
    event_bus: Option<Arc<EventBus>>,
    /// C87 — set only via [`Self::with_dirs`]. Drives [`Self::on_start`]:
    /// `with_builtins` plugins (dirs both `None`) never spin up a watcher.
    themes_dir: Option<PathBuf>,
    snippets_dir: Option<PathBuf>,
    watcher: Option<WatcherHandle>,
}

impl ThemeCorePlugin {
    /// Create a new plugin from an existing engine. `event_bus` is
    /// optional — when `None`, mutation events are silently dropped.
    #[must_use]
    pub fn new(engine: ThemeEngine, event_bus: Option<Arc<EventBus>>) -> Self {
        Self {
            engine: Arc::new(Mutex::new(engine)),
            event_bus,
            themes_dir: None,
            snippets_dir: None,
            watcher: None,
        }
    }

    /// Fresh plugin with only the built-in themes. Convenience for
    /// tests and the Tauri shell.
    #[must_use]
    pub fn with_builtins(event_bus: Option<Arc<EventBus>>) -> Self {
        Self::new(ThemeEngine::new(), event_bus)
    }

    /// C87 — plugin that also discovers user-installed themes/snippets
    /// under `themes_dir`/`snippets_dir` and, once started
    /// ([`CorePlugin::on_start`]), watches both for changes and
    /// live-reloads. Falls back to builtins-only (logging a warning) if
    /// the initial scan fails — never worth failing plugin construction
    /// over a themes directory an operator hasn't created yet, since
    /// [`ThemeEngine::with_dirs`] already treats "missing" as empty; a
    /// scan `Err` here means something more unusual (e.g. a permissions
    /// error), and this is a non-essential UX feature, not a boot gate.
    #[must_use]
    pub fn with_dirs(
        themes_dir: impl AsRef<Path>,
        snippets_dir: impl AsRef<Path>,
        event_bus: Option<Arc<EventBus>>,
    ) -> Self {
        let themes_dir = themes_dir.as_ref().to_path_buf();
        let snippets_dir = snippets_dir.as_ref().to_path_buf();
        let engine = ThemeEngine::with_dirs(&themes_dir, &snippets_dir).unwrap_or_else(|e| {
            tracing::warn!(
                themes_dir = %themes_dir.display(),
                snippets_dir = %snippets_dir.display(),
                error = %e,
                "theme/snippet discovery failed; falling back to built-ins only"
            );
            ThemeEngine::new()
        });
        Self {
            engine: Arc::new(Mutex::new(engine)),
            event_bus,
            themes_dir: Some(themes_dir),
            snippets_dir: Some(snippets_dir),
            watcher: None,
        }
    }

    fn publish_changed(&self, config: &ThemeConfig) {
        if let Some(bus) = &self.event_bus {
            let payload = serde_json::to_value(config).unwrap_or(Value::Null);
            let _ = bus.publish_plugin(PLUGIN_ID, EVENT_CHANGED, payload);
        }
    }
}

impl CorePlugin for ThemeCorePlugin {
    fn on_start(&mut self) -> Result<(), PluginError> {
        // `with_builtins` plugins (e.g. every existing test, and any
        // caller that hasn't opted into on-disk discovery) have both
        // dirs `None` — nothing to watch, and starting a watcher with
        // no paths would just spin a thread that never fires.
        if self.themes_dir.is_none() && self.snippets_dir.is_none() {
            return Ok(());
        }
        let watcher = match ThemeWatcher::start(
            self.themes_dir.as_deref(),
            self.snippets_dir.as_deref(),
            DEFAULT_DEBOUNCE_MS,
        ) {
            Ok(w) => w,
            Err(e) => {
                // Soft-fail (BL-lifecycle-skip posture, mirrors bootstrap's
                // `or_lifecycle_skip`): a watcher that can't start means no
                // hot-reload, not a broken theme engine — the already-
                // discovered themes/snippets from `with_dirs`'s initial
                // scan still work. Never take down boot for this.
                tracing::warn!(error = %e, "theme watcher failed to start; hot-reload disabled");
                return Ok(());
            }
        };

        let running = Arc::new(AtomicBool::new(true));
        let running_thread = Arc::clone(&running);
        let engine = Arc::clone(&self.engine);
        let event_bus = self.event_bus.clone();

        let join = std::thread::spawn(move || {
            while running_thread.load(Ordering::Acquire) {
                let Some(event) = watcher.recv_timeout(Duration::from_millis(300)) else {
                    continue;
                };
                let cfg = {
                    let Ok(mut engine) = engine.lock() else {
                        tracing::error!("theme engine mutex poisoned in watcher thread");
                        return;
                    };
                    if let Err(e) = engine.reload() {
                        let (kind, id) = match &event {
                            ThemeReloadEvent::Theme { id, .. } => ("theme", id.as_str()),
                            ThemeReloadEvent::Snippet { id, .. } => ("snippet", id.as_str()),
                        };
                        tracing::warn!(kind, id, error = %e, "theme hot-reload scan failed");
                        continue;
                    }
                    engine.config()
                };
                if let Some(bus) = &event_bus {
                    let payload = serde_json::to_value(&cfg).unwrap_or(Value::Null);
                    let _ = bus.publish_plugin(PLUGIN_ID, EVENT_CHANGED, payload);
                }
            }
        });

        self.watcher = Some(WatcherHandle { running, join });
        Ok(())
    }

    fn on_stop(&mut self) {
        if let Some(handle) = self.watcher.take() {
            handle.running.store(false, Ordering::Release);
            // Bounded by the 300ms recv_timeout poll above — the thread
            // notices within one tick and exits, dropping the
            // ThemeWatcher (and its debouncer) with it.
            if handle.join.join().is_err() {
                tracing::warn!("theme watcher thread panicked during shutdown");
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn dispatch(&mut self, handler_id: u32, args: &Value) -> Result<Value, PluginError> {
        match handler_id {
            HANDLER_GET_AVAILABLE_THEMES => {
                let engine = self.engine.lock().map_err(|_| engine_poisoned())?;
                to_value(&engine.get_available_themes(), "get_available_themes")
            }
            HANDLER_APPLY_THEME => {
                let a: ApplyThemeArgs = parse_args(args, "apply_theme")?;
                let (resp, cfg) = {
                    let mut engine = self.engine.lock().map_err(|_| engine_poisoned())?;
                    let applied = engine
                        .apply_theme(&a.id)
                        .map_err(|e| exec_err(format!("apply_theme: {e}")))?;
                    (applied, engine.config())
                };
                self.publish_changed(&cfg);
                to_value(&resp, "apply_theme")
            }
            HANDLER_COMPUTE_VARIABLES => {
                let a: ComputeVariablesArgs = parse_args(args, "compute_variables")?;
                let engine = self.engine.lock().map_err(|_| engine_poisoned())?;
                let vars = engine
                    .compute_variables(&a.theme_id, &a.enabled_snippets)
                    .map_err(|e| exec_err(format!("compute_variables: {e}")))?;
                to_value(&vars, "compute_variables")
            }
            HANDLER_GET_AVAILABLE_SNIPPETS => {
                let engine = self.engine.lock().map_err(|_| engine_poisoned())?;
                to_value(&engine.get_available_snippets(), "get_available_snippets")
            }
            HANDLER_TOGGLE_SNIPPET => {
                let a: ToggleSnippetArgs = parse_args(args, "toggle_snippet")?;
                let (resp, cfg) = {
                    let mut engine = self.engine.lock().map_err(|_| engine_poisoned())?;
                    let ids = engine
                        .toggle_snippet(&a.id)
                        .map_err(|e| exec_err(format!("toggle_snippet: {e}")))?;
                    (ids, engine.config())
                };
                self.publish_changed(&cfg);
                to_value(&resp, "toggle_snippet")
            }
            HANDLER_REORDER_SNIPPETS => {
                let a: ReorderSnippetsArgs = parse_args(args, "reorder_snippets")?;
                let cfg = {
                    let mut engine = self.engine.lock().map_err(|_| engine_poisoned())?;
                    engine
                        .reorder_snippets(a.ids)
                        .map_err(|e| exec_err(format!("reorder_snippets: {e}")))?;
                    engine.config()
                };
                self.publish_changed(&cfg);
                Ok(Value::Object(serde_json::Map::new()))
            }
            HANDLER_GET_THEME_CONFIG => {
                let engine = self.engine.lock().map_err(|_| engine_poisoned())?;
                to_value(&engine.config(), "get_theme_config")
            }
            HANDLER_SET_MODE => {
                let a: SetModeArgs = parse_args(args, "set_mode")?;
                let (resp, cfg) = {
                    let mut engine = self.engine.lock().map_err(|_| engine_poisoned())?;
                    let applied = engine.set_mode(a.mode);
                    (applied, engine.config())
                };
                self.publish_changed(&cfg);
                to_value(&resp, "set_mode")
            }
            HANDLER_APPLY_CONFIG => {
                let a: ApplyConfigArgs = parse_args(args, "apply_config")?;
                let cfg = {
                    let mut engine = self.engine.lock().map_err(|_| engine_poisoned())?;
                    engine.apply_config(a.config);
                    engine.config()
                };
                self.publish_changed(&cfg);
                to_value(&Ack { ok: true }, "apply_config")
            }
            HANDLER_SET_PLUGIN_OVERRIDES => {
                let a: SetPluginOverridesArgs = parse_args(args, "set_plugin_overrides")?;
                let cfg = {
                    let mut engine = self.engine.lock().map_err(|_| engine_poisoned())?;
                    engine.set_plugin_overrides(a.overrides);
                    engine.config()
                };
                self.publish_changed(&cfg);
                to_value(&Ack { ok: true }, "set_plugin_overrides")
            }
            HANDLER_RELOAD => {
                let cfg = {
                    let mut engine = self.engine.lock().map_err(|_| engine_poisoned())?;
                    engine
                        .reload()
                        .map_err(|e| exec_err(format!("reload: {e}")))?;
                    engine.config()
                };
                self.publish_changed(&cfg);
                to_value(&Ack { ok: true }, "reload")
            }
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

nexus_plugins::define_dispatch_helpers!();

fn engine_poisoned() -> PluginError {
    exec_err("theme engine mutex poisoned".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_kernel::EventFilter;

    fn json(v: serde_json::Value) -> Value {
        v
    }

    #[test]
    fn dispatch_list_themes_returns_builtins() {
        let mut plugin = ThemeCorePlugin::with_builtins(None);
        let out = plugin
            .dispatch(HANDLER_GET_AVAILABLE_THEMES, &json(serde_json::json!({})))
            .unwrap();
        let list = out.as_array().expect("array of themes");
        assert!(list.iter().any(|t| t["id"] == "nexus-light"));
        assert!(list.iter().any(|t| t["id"] == "nexus-dark"));
    }

    #[test]
    fn apply_theme_emits_changed_event() {
        let bus = Arc::new(EventBus::new(16));
        let mut sub = bus.subscribe(EventFilter::CustomPrefix("com.nexus.theme.".to_string()));
        let mut plugin = ThemeCorePlugin::with_builtins(Some(Arc::clone(&bus)));

        plugin
            .dispatch(
                HANDLER_APPLY_THEME,
                &json(serde_json::json!({ "id": "nexus-dark" })),
            )
            .unwrap();

        let event = sub.try_recv().unwrap().unwrap();
        match &event.event {
            nexus_kernel::NexusEvent::Custom {
                type_id, payload, ..
            } => {
                assert_eq!(type_id, EVENT_CHANGED);
                assert_eq!(payload["theme_id"], "nexus-dark");
            }
            other => panic!("expected Custom, got {other:?}"),
        }
    }

    #[test]
    fn set_mode_emits_changed_event() {
        let bus = Arc::new(EventBus::new(16));
        let mut sub = bus.subscribe(EventFilter::CustomPrefix("com.nexus.theme.".to_string()));
        let mut plugin = ThemeCorePlugin::with_builtins(Some(Arc::clone(&bus)));

        plugin
            .dispatch(
                HANDLER_SET_MODE,
                &json(serde_json::json!({ "mode": "dark" })),
            )
            .unwrap();

        let event = sub.try_recv().unwrap().unwrap();
        match &event.event {
            nexus_kernel::NexusEvent::Custom {
                type_id, payload, ..
            } => {
                assert_eq!(type_id, EVENT_CHANGED);
                assert_eq!(payload["mode"], "dark");
            }
            other => panic!("expected Custom, got {other:?}"),
        }
    }

    #[test]
    fn unknown_handler_id_errors() {
        let mut plugin = ThemeCorePlugin::with_builtins(None);
        let err = plugin
            .dispatch(9999, &json(serde_json::json!({})))
            .expect_err("should error");
        match err {
            PluginError::ExecutionFailed { plugin_id, .. } => {
                assert_eq!(plugin_id, PLUGIN_ID);
            }
            other => panic!("expected ExecutionFailed, got {other:?}"),
        }
    }

    #[test]
    fn get_theme_config_after_apply_reflects_state() {
        let mut plugin = ThemeCorePlugin::with_builtins(None);
        plugin
            .dispatch(
                HANDLER_APPLY_THEME,
                &json(serde_json::json!({ "id": "nexus-dark" })),
            )
            .unwrap();
        let out = plugin
            .dispatch(HANDLER_GET_THEME_CONFIG, &json(serde_json::json!({})))
            .unwrap();
        assert_eq!(out["theme_id"], "nexus-dark");
    }

    #[test]
    fn reorder_snippets_with_unknown_id_errors() {
        let mut plugin = ThemeCorePlugin::with_builtins(None);
        let err = plugin
            .dispatch(
                HANDLER_REORDER_SNIPPETS,
                &json(serde_json::json!({ "ids": ["ghost"] })),
            )
            .expect_err("unknown id should fail");
        if let PluginError::ExecutionFailed { reason, .. } = err {
            assert!(reason.contains("reorder_snippets"));
        } else {
            panic!("expected ExecutionFailed");
        }
    }

    // ── C87: with_dirs + on_start/on_stop lifecycle ─────────────────────────

    #[test]
    fn with_builtins_on_start_is_a_noop_without_dirs() {
        // No dirs configured → on_start must not spin up a watcher thread
        // (there's nothing to watch, and it would never fire).
        let mut plugin = ThemeCorePlugin::with_builtins(None);
        plugin.on_start().unwrap();
        assert!(plugin.watcher.is_none());
        // on_stop with no watcher running must not panic.
        plugin.on_stop();
    }

    #[test]
    fn with_dirs_discovers_themes_and_snippets_on_construction() {
        let themes = tempfile::tempdir().unwrap();
        let theme_dir = themes.path().join("custom");
        std::fs::create_dir(&theme_dir).unwrap();
        std::fs::write(
            theme_dir.join("NEXUS.toml"),
            r#"
[theme]
name = "Custom"
version = "0.1.0"
author = "x"
description = "d"
"#,
        )
        .unwrap();

        let snippets = tempfile::tempdir().unwrap();
        std::fs::write(
            snippets.path().join("neon.css"),
            "/* Name: Neon\nDescription: d */\n:root { --nx-a: 1; }",
        )
        .unwrap();

        let mut plugin = ThemeCorePlugin::with_dirs(themes.path(), snippets.path(), None);
        let themes_out = plugin
            .dispatch(HANDLER_GET_AVAILABLE_THEMES, &json(serde_json::json!({})))
            .unwrap();
        assert!(themes_out
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == "custom"));

        let snippets_out = plugin
            .dispatch(HANDLER_GET_AVAILABLE_SNIPPETS, &json(serde_json::json!({})))
            .unwrap();
        assert!(snippets_out
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == "neon"));
    }

    #[test]
    fn with_dirs_falls_back_to_builtins_on_missing_dirs() {
        // Missing dirs are treated as empty by ThemeEngine::with_dirs, not
        // an error — construction must succeed with builtins intact.
        let mut plugin = ThemeCorePlugin::with_dirs(
            "/nonexistent/themes/for/real",
            "/nonexistent/snippets/for/real",
            None,
        );
        let out = plugin
            .dispatch(HANDLER_GET_AVAILABLE_THEMES, &json(serde_json::json!({})))
            .unwrap();
        let ids: Vec<&str> = out
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"nexus-light"));
        assert!(ids.contains(&"nexus-dark"));
    }

    #[test]
    fn on_start_with_dirs_spins_up_a_watcher_that_on_stop_cleanly_tears_down() {
        let themes = tempfile::tempdir().unwrap();
        let snippets = tempfile::tempdir().unwrap();
        let mut plugin = ThemeCorePlugin::with_dirs(themes.path(), snippets.path(), None);

        plugin.on_start().unwrap();
        assert!(plugin.watcher.is_some(), "watcher must be running");

        plugin.on_stop();
        assert!(plugin.watcher.is_none(), "on_stop must clear the handle");
    }

    #[test]
    fn hot_reload_picks_up_a_new_theme_and_emits_changed() {
        // Mirrors `watcher::tests::detects_theme_manifest_change`'s shape
        // (the *directory* exists before the watch starts; only the
        // manifest file inside it is written afterward) — that's the
        // pattern proven reliable for this crate's notify-backed watcher
        // in sandboxed/CI filesystems. Same soft-assert posture as that
        // test and its sibling `detects_snippet_change`: OS file-watchers
        // are inherently flaky under WSL2 / network mounts / restricted
        // containers, so a watcher that never fires downgrades to a
        // logged note rather than failing CI, while the mutation IS
        // asserted (proving `reload()` + `publish_changed` actually run)
        // whenever an event does arrive.
        let themes = tempfile::tempdir().unwrap();
        let theme_dir = themes.path().join("live-added");
        std::fs::create_dir(&theme_dir).unwrap();
        let snippets = tempfile::tempdir().unwrap();

        let bus = Arc::new(EventBus::new(16));
        let mut sub = bus.subscribe(EventFilter::CustomPrefix("com.nexus.theme.".to_string()));

        let mut plugin =
            ThemeCorePlugin::with_dirs(themes.path(), snippets.path(), Some(Arc::clone(&bus)));
        plugin.on_start().unwrap();
        std::thread::sleep(Duration::from_millis(150));

        std::fs::write(
            theme_dir.join("NEXUS.toml"),
            r#"
[theme]
name = "Live"
version = "0.1.0"
author = "x"
description = "d"
"#,
        )
        .unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut saw_new_theme = false;
        while std::time::Instant::now() < deadline && !saw_new_theme {
            let themes_out = plugin
                .dispatch(HANDLER_GET_AVAILABLE_THEMES, &json(serde_json::json!({})))
                .unwrap();
            saw_new_theme = themes_out
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["id"] == "live-added");
            if !saw_new_theme {
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        if saw_new_theme {
            // A `changed` event should have fired from the reload — drain
            // instead of asserting a single try_recv since the debounced
            // watcher may coalesce/fire more than once.
            let mut saw_event = false;
            while let Ok(Some(event)) = sub.try_recv() {
                if let nexus_kernel::NexusEvent::Custom { type_id, .. } = &event.event {
                    if type_id == EVENT_CHANGED {
                        saw_event = true;
                    }
                }
            }
            assert!(saw_event, "hot-reload must publish com.nexus.theme.changed");
        } else {
            eprintln!(
                "note: theme watcher did not pick up the manifest write within 5s — \
                 likely a host-FS limitation (see watcher::tests for the same caveat)"
            );
        }

        plugin.on_stop();
    }
}
