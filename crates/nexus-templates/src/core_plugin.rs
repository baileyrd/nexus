//! Core plugin wrapping [`TemplateRegistry`].
//!
//! Exposes templates over kernel IPC so the shell, CLI plugin frontends,
//! and community plugins can list and apply templates without linking
//! `nexus-templates` directly.
//!
//! # Handlers
//!
//! | Id | Command  | Args                                            | Purpose                                       |
//! |---:|----------|-------------------------------------------------|-----------------------------------------------|
//! | 1  | `list`   | `{}`                                            | Every template in the registry (sorted).      |
//! | 2  | `get`    | `{ name }`                                      | One template by name; 404 if missing.         |
//! | 3  | `render` | `{ name, args?: {…} }`                          | Dry-run render — returns body + target path.  |
//! | 4  | `apply`  | `{ name, args?: {…}, target?, overwrite? }`    | Render and write to disk; returns the path.   |
//! | 5  | `reload` | `{}`                                            | Re-scan `<forge>/.forge/templates`.           |
//!
//! Ids are append-only.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use nexus_plugins::{CorePlugin, PluginError};
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::{TemplateRegistry, TemplateRegistryError};

// ── IPC arg types ──────────────────────────────────────────────────────────

/// Args for `com.nexus.templates::get` (handler `2`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct GetPageTemplateArgs {
    /// Unique short template name (matches the `name:` frontmatter field).
    pub name: String,
}

/// Args for `com.nexus.templates::render` (handler `3`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct RenderTemplateArgs {
    /// Template name to render.
    pub name: String,
    /// Parameter overrides. Each value is a string (the substitution engine
    /// only handles strings; richer types render via JSON-stringify on the
    /// caller side).
    #[serde(default)]
    pub args: BTreeMap<String, String>,
}

/// Args for `com.nexus.templates::apply` (handler `4`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ApplyTemplateArgs {
    /// Template name to apply.
    pub name: String,
    /// Parameter overrides.
    #[serde(default)]
    pub args: BTreeMap<String, String>,
    /// Override the template's `target_path` (forge-relative). Optional.
    #[serde(default)]
    pub target: Option<String>,
    /// If `false` (default), refuse to overwrite an existing file. If
    /// `true`, overwrite freely.
    #[serde(default)]
    pub overwrite: bool,
}

// ── Handler ids and plugin id ──────────────────────────────────────────────

/// Reverse-DNS plugin id.
pub const PLUGIN_ID: &str = "com.nexus.templates";

/// `list` handler id.
pub const HANDLER_LIST: u32 = 1;
/// `get` handler id.
pub const HANDLER_GET: u32 = 2;
/// `render` (dry-run) handler id.
pub const HANDLER_RENDER: u32 = 3;
/// `apply` handler id.
pub const HANDLER_APPLY: u32 = 4;
/// `reload` handler id.
pub const HANDLER_RELOAD: u32 = 5;

// ── Plugin ─────────────────────────────────────────────────────────────────

/// Core plugin — holds the forge root + a registry behind a mutex so
/// dispatches stay `Send + Sync`.
pub struct TemplatesCorePlugin {
    forge_root: PathBuf,
    registry: Mutex<TemplateRegistry>,
}

impl TemplatesCorePlugin {
    /// Build a plugin against the given forge root. Loads built-ins +
    /// any user templates eagerly. Load failures degrade to an empty
    /// registry with a `warn!` log.
    #[must_use]
    pub fn open(forge_root: PathBuf) -> Self {
        let registry = match TemplateRegistry::load(&forge_root) {
            Ok(reg) => reg,
            Err(err) => {
                tracing::warn!(
                    path = %forge_root.display(),
                    %err,
                    "com.nexus.templates: load failed; registry starts empty"
                );
                TemplateRegistry::empty()
            }
        };
        Self {
            forge_root,
            registry: Mutex::new(registry),
        }
    }
}

impl CorePlugin for TemplatesCorePlugin {
    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        match handler_id {
            HANDLER_LIST => self.dispatch_list(),
            HANDLER_GET => self.dispatch_get(args),
            HANDLER_RENDER => self.dispatch_render(args),
            HANDLER_APPLY => self.dispatch_apply(args),
            HANDLER_RELOAD => self.dispatch_reload(),
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }
}

impl TemplatesCorePlugin {
    fn dispatch_list(&self) -> Result<serde_json::Value, PluginError> {
        let reg = self.registry.lock().map_err(poisoned)?;
        let items: Vec<serde_json::Value> = reg
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.meta.name,
                    "description": t.meta.description,
                    "target_path": t.meta.target_path,
                    "parameters": t.meta.parameters,
                })
            })
            .collect();
        // Stable sort for determinism.
        let mut items = items;
        items.sort_by(|a, b| {
            a.get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .cmp(b.get("name").and_then(|v| v.as_str()).unwrap_or(""))
        });
        Ok(serde_json::Value::Array(items))
    }

    fn dispatch_get(&self, args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
        let a: GetPageTemplateArgs = parse_args(args, "get")?;
        let reg = self.registry.lock().map_err(poisoned)?;
        let tpl = reg
            .get(&a.name)
            .ok_or_else(|| exec_err(format!("no template named '{}'", a.name)))?;
        to_value(tpl, "get")
    }

    fn dispatch_render(&self, args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
        let a: RenderTemplateArgs = parse_args(args, "render")?;
        let reg = self.registry.lock().map_err(poisoned)?;
        let tpl = reg
            .get(&a.name)
            .ok_or_else(|| exec_err(format!("no template named '{}'", a.name)))?;
        let values = tpl
            .resolve_values(&a.args, &self.forge_root)
            .map_err(|e| exec_err(format!("render: {e}")))?;
        let (body, target) = tpl
            .render(&values)
            .map_err(|e| exec_err(format!("render: {e}")))?;
        Ok(serde_json::json!({
            "name": tpl.meta.name,
            "target_path": target,
            "body": body,
        }))
    }

    fn dispatch_apply(&self, args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
        let a: ApplyTemplateArgs = parse_args(args, "apply")?;
        let reg = self.registry.lock().map_err(poisoned)?;
        let tpl = reg
            .get(&a.name)
            .ok_or_else(|| exec_err(format!("no template named '{}'", a.name)))?;

        // CLI-style target override: stash into args under a reserved key
        // and override target_path manually if provided. Cleanest approach
        // is to build a per-call template clone with the target patched.
        let mut effective = tpl.clone();
        if let Some(t) = &a.target {
            effective.meta.target_path = Some(t.clone());
        }

        let written = effective
            .apply(&a.args, &self.forge_root, a.overwrite)
            .map_err(|e| exec_err(format!("apply: {e}")))?;
        let rel = written
            .strip_prefix(&self.forge_root)
            .unwrap_or(&written)
            .display()
            .to_string();
        Ok(serde_json::json!({
            "name": tpl.meta.name,
            "path": rel,
            "absolute_path": written.display().to_string(),
        }))
    }

    fn dispatch_reload(&self) -> Result<serde_json::Value, PluginError> {
        let reloaded = TemplateRegistry::load(&self.forge_root).unwrap_or_else(|err| {
            tracing::warn!(
                path = %self.forge_root.display(),
                %err,
                "com.nexus.templates reload: full failure; registry starts empty"
            );
            TemplateRegistry::empty()
        });
        let len = reloaded.len();
        *self.registry.lock().map_err(poisoned)? = reloaded;
        Ok(serde_json::json!({ "loaded": len }))
    }
}

// ── Plumbing ───────────────────────────────────────────────────────────────

nexus_plugins::define_dispatch_helpers!();

fn poisoned<T>(_e: std::sync::PoisonError<T>) -> PluginError {
    exec_err("templates registry mutex poisoned — prior handler panicked".to_string())
}

// `TemplateRegistryError` is exposed transitively for callers building
// their own registry; suppress the unused-import warning when no caller
// references it from this module.
#[allow(dead_code)]
fn _silence_unused_imports(_: &TemplateRegistryError) {}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn open_plugin() -> (TemplatesCorePlugin, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let plugin = TemplatesCorePlugin::open(dir.path().to_path_buf());
        (plugin, dir)
    }

    #[test]
    fn list_returns_built_ins() {
        let (mut plugin, _dir) = open_plugin();
        let result = plugin
            .dispatch(HANDLER_LIST, &serde_json::json!({}))
            .unwrap();
        let arr = result.as_array().expect("array");
        assert!(arr.len() >= 4, "got {arr:?}");
    }

    #[test]
    fn get_returns_a_known_builtin() {
        let (mut plugin, _dir) = open_plugin();
        let result = plugin
            .dispatch(HANDLER_GET, &serde_json::json!({ "name": "daily-journal" }))
            .unwrap();
        // TemplateMeta is `#[serde(flatten)]` into Template, so `name`
        // is at the top level.
        assert_eq!(result.get("name").and_then(|v| v.as_str()), Some("daily-journal"));
    }

    #[test]
    fn get_unknown_template_errors() {
        let (mut plugin, _dir) = open_plugin();
        let err = plugin
            .dispatch(HANDLER_GET, &serde_json::json!({ "name": "nope" }))
            .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("nope"), "{msg}");
    }

    #[test]
    fn render_dry_runs_a_template() {
        let (mut plugin, _dir) = open_plugin();
        let mut args = serde_json::Map::new();
        args.insert("name".into(), serde_json::Value::String("daily-journal".into()));
        let result = plugin
            .dispatch(HANDLER_RENDER, &serde_json::Value::Object(args))
            .unwrap();
        assert!(result.get("body").is_some());
        assert!(result.get("target_path").is_some());
        let target = result["target_path"].as_str().unwrap();
        assert!(target.starts_with("daily/"), "{target}");
    }

    #[test]
    fn apply_writes_a_file() {
        let (mut plugin, dir) = open_plugin();
        let result = plugin
            .dispatch(
                HANDLER_APPLY,
                &serde_json::json!({
                    "name": "notion-page",
                    "args": { "title": "Hello", "status": "draft", "tags": "" }
                }),
            )
            .unwrap();
        let abs = result["absolute_path"].as_str().unwrap();
        assert!(std::path::Path::new(abs).exists(), "missing: {abs}");
        let body = std::fs::read_to_string(dir.path().join("Hello.md")).unwrap();
        assert!(body.contains("# Hello"), "{body}");
    }

    #[test]
    fn apply_with_target_override() {
        let (mut plugin, dir) = open_plugin();
        plugin
            .dispatch(
                HANDLER_APPLY,
                &serde_json::json!({
                    "name": "notion-page",
                    "args": { "title": "X" },
                    "target": "custom/path.md"
                }),
            )
            .unwrap();
        assert!(dir.path().join("custom/path.md").exists());
    }

    #[test]
    fn apply_refuses_overwrite_by_default() {
        let (mut plugin, _dir) = open_plugin();
        plugin
            .dispatch(
                HANDLER_APPLY,
                &serde_json::json!({ "name": "notion-page", "args": { "title": "Dup" } }),
            )
            .unwrap();
        let err = plugin
            .dispatch(
                HANDLER_APPLY,
                &serde_json::json!({ "name": "notion-page", "args": { "title": "Dup" } }),
            )
            .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.to_lowercase().contains("already"), "{msg}");
    }

    #[test]
    fn reload_picks_up_new_user_template() {
        let (mut plugin, dir) = open_plugin();
        let templates_dir = dir.path().join(".forge/templates");
        std::fs::create_dir_all(&templates_dir).unwrap();
        std::fs::write(
            templates_dir.join("custom.template.md"),
            "---\nname: my-custom\n---\nHello.\n",
        )
        .unwrap();

        let result = plugin
            .dispatch(HANDLER_RELOAD, &serde_json::json!({}))
            .unwrap();
        assert!(result["loaded"].as_u64().unwrap() >= 5);

        let g = plugin
            .dispatch(HANDLER_GET, &serde_json::json!({ "name": "my-custom" }))
            .unwrap();
        assert!(g.get("body").is_some());
    }
}
