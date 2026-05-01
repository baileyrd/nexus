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

use std::sync::{Arc, Mutex};

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

/// Core plugin wrapping a [`ThemeEngine`] behind a mutex and an
/// [`EventBus`] hook for mutation events.
pub struct ThemeCorePlugin {
    engine: Arc<Mutex<ThemeEngine>>,
    event_bus: Option<Arc<EventBus>>,
}

impl ThemeCorePlugin {
    /// Create a new plugin from an existing engine. `event_bus` is
    /// optional — when `None`, mutation events are silently dropped.
    #[must_use]
    pub fn new(engine: ThemeEngine, event_bus: Option<Arc<EventBus>>) -> Self {
        Self {
            engine: Arc::new(Mutex::new(engine)),
            event_bus,
        }
    }

    /// Fresh plugin with only the built-in themes. Convenience for
    /// tests and the Tauri shell.
    #[must_use]
    pub fn with_builtins(event_bus: Option<Arc<EventBus>>) -> Self {
        Self::new(ThemeEngine::new(), event_bus)
    }

    fn publish_changed(&self, config: &ThemeConfig) {
        if let Some(bus) = &self.event_bus {
            let payload = serde_json::to_value(config).unwrap_or(Value::Null);
            let _ = bus.publish_plugin(PLUGIN_ID, EVENT_CHANGED, payload);
        }
    }
}

impl CorePlugin for ThemeCorePlugin {
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
                to_value(
                    &engine.get_available_snippets(),
                    "get_available_snippets",
                )
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

fn exec_err(reason: String) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason,
    }
}

fn engine_poisoned() -> PluginError {
    exec_err("theme engine mutex poisoned".to_string())
}

fn parse_args<T: serde::de::DeserializeOwned>(
    value: &Value,
    command: &str,
) -> Result<T, PluginError> {
    serde_json::from_value(value.clone())
        .map_err(|e| exec_err(format!("{command}: invalid args: {e}")))
}

fn to_value<T: serde::Serialize>(v: &T, command: &str) -> Result<Value, PluginError> {
    serde_json::to_value(v).map_err(|e| exec_err(format!("{command}: serialize failed: {e}")))
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
        let mut sub =
            bus.subscribe(EventFilter::CustomPrefix("com.nexus.theme.".to_string()));
        let mut plugin = ThemeCorePlugin::with_builtins(Some(Arc::clone(&bus)));

        plugin
            .dispatch(
                HANDLER_APPLY_THEME,
                &json(serde_json::json!({ "id": "nexus-dark" })),
            )
            .unwrap();

        let event = sub.try_recv().unwrap().unwrap();
        match &event.event {
            nexus_kernel::NexusEvent::Custom { type_id, payload, .. } => {
                assert_eq!(type_id, EVENT_CHANGED);
                assert_eq!(payload["theme_id"], "nexus-dark");
            }
            other => panic!("expected Custom, got {other:?}"),
        }
    }

    #[test]
    fn set_mode_emits_changed_event() {
        let bus = Arc::new(EventBus::new(16));
        let mut sub =
            bus.subscribe(EventFilter::CustomPrefix("com.nexus.theme.".to_string()));
        let mut plugin = ThemeCorePlugin::with_builtins(Some(Arc::clone(&bus)));

        plugin
            .dispatch(
                HANDLER_SET_MODE,
                &json(serde_json::json!({ "mode": "dark" })),
            )
            .unwrap();

        let event = sub.try_recv().unwrap().unwrap();
        match &event.event {
            nexus_kernel::NexusEvent::Custom { type_id, payload, .. } => {
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
}
