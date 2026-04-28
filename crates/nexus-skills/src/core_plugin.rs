//! Core plugin wrapping [`SkillRegistry`].
//!
//! Exposes the registry over kernel IPC so the agent planner + Chat
//! panel + future Workflow system can consult available skills
//! without linking `nexus-skills` directly. Plugin is *load-only* —
//! mutations happen by editing `.skill.md` files and calling
//! `reload`, not by writing through IPC.
//!
//! # Handlers
//!
//! | Id | Command            | Args               | Purpose                                  |
//! |---:|--------------------|--------------------|------------------------------------------|
//! | 1  | `list`             | `{}`               | Every loaded skill                       |
//! | 2  | `get`              | `{ id }`           | One skill by id (404 if missing)         |
//! | 3  | `list_by_context`  | `{ context }`      | Skills whose `applicable_contexts` match |
//! | 4  | `triggered_by`     | `{ text }`         | Skills whose trigger matches `text`      |
//! | 5  | `reload`           | `{}`               | Re-scan the `<forge>/.forge/skills` dir  |
//! | 6  | `render`           | `{ id, values? }`  | Render a skill's body with parameter substitution |
//! | 7  | `compose`          | `{ id }`           | BL-021 — resolve `depends_on` closure into ordered fragments + merged body |
//!
//! Ids are append-only.

use std::path::PathBuf;
use std::sync::Mutex;

use nexus_plugins::{CorePlugin, PluginError};
use serde::Deserialize;

use crate::{registry_index, SkillRegistry, SkillRegistryError};

/// Reverse-DNS identifier.
pub const PLUGIN_ID: &str = "com.nexus.skills";

/// `list` handler id.
pub const HANDLER_LIST: u32 = 1;
/// `get` handler id.
pub const HANDLER_GET: u32 = 2;
/// `list_by_context` handler id.
pub const HANDLER_LIST_BY_CONTEXT: u32 = 3;
/// `triggered_by` handler id.
pub const HANDLER_TRIGGERED_BY: u32 = 4;
/// `reload` handler id.
pub const HANDLER_RELOAD: u32 = 5;
/// `render` handler id.
pub const HANDLER_RENDER: u32 = 6;
/// `compose` handler id (BL-021).
pub const HANDLER_COMPOSE: u32 = 7;

/// Core plugin — holds the skills root path + an in-memory registry
/// behind a mutex so dispatches stay `Send + Sync`.
pub struct SkillsCorePlugin {
    root: PathBuf,
    registry: Mutex<SkillRegistry>,
}

impl SkillsCorePlugin {
    /// Construct with the forge's `.forge/skills` directory.
    /// Eagerly loads the registry; partial parse failures are
    /// logged at `warn` and the registry starts with whatever did
    /// parse. Callers that want stricter startup can call `reload`
    /// themselves and inspect the error.
    #[must_use]
    pub fn open(skills_dir: PathBuf) -> Self {
        let registry = match SkillRegistry::load(&skills_dir) {
            Ok(reg) => reg,
            Err(SkillRegistryError::PartialParseFailure { count, first }) => {
                tracing::warn!(
                    path = %skills_dir.display(),
                    count,
                    first = %first,
                    "com.nexus.skills: {count} skill file(s) failed to parse during load"
                );
                SkillRegistry::empty()
            }
            Err(err) => {
                tracing::warn!(
                    path = %skills_dir.display(),
                    err = %err,
                    "com.nexus.skills: load failed; registry starts empty"
                );
                SkillRegistry::empty()
            }
        };
        // Best-effort: persist the on-disk REGISTRY.json index so
        // external CLIs can cold-start without a directory walk.
        // PRD-13 §3.1. Failures must not block plugin open.
        let index_path = skills_dir.join("REGISTRY.json");
        if let Err(err) = registry_index::write_index(&index_path, &skills_dir, &registry) {
            tracing::warn!(
                path = %index_path.display(),
                err = %err,
                "com.nexus.skills: failed to persist REGISTRY.json on open"
            );
        }
        Self {
            root: skills_dir,
            registry: Mutex::new(registry),
        }
    }
}

impl CorePlugin for SkillsCorePlugin {
    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        match handler_id {
            HANDLER_LIST => self.dispatch_list(),
            HANDLER_GET => self.dispatch_get(args),
            HANDLER_LIST_BY_CONTEXT => self.dispatch_list_by_context(args),
            HANDLER_TRIGGERED_BY => self.dispatch_triggered_by(args),
            HANDLER_RELOAD => self.dispatch_reload(),
            HANDLER_RENDER => self.dispatch_render(args),
            HANDLER_COMPOSE => self.dispatch_compose(args),
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }
}

impl SkillsCorePlugin {
    fn dispatch_list(&self) -> Result<serde_json::Value, PluginError> {
        let reg = self.registry.lock().map_err(poisoned)?;
        let skills: Vec<_> = reg.iter().cloned().collect();
        to_value(&skills, "list")
    }

    fn dispatch_get(&self, args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
        #[derive(Deserialize)]
        struct Args {
            id: String,
        }
        let a: Args = parse(args, "get")?;
        let reg = self.registry.lock().map_err(poisoned)?;
        match reg.get(&a.id) {
            Some(skill) => to_value(skill, "get"),
            None => Err(exec_err(format!("no skill with id '{}'", a.id))),
        }
    }

    fn dispatch_list_by_context(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        #[derive(Deserialize)]
        struct Args {
            context: String,
        }
        let a: Args = parse(args, "list_by_context")?;
        let reg = self.registry.lock().map_err(poisoned)?;
        let skills: Vec<_> = reg.by_context(&a.context).cloned().collect();
        to_value(&skills, "list_by_context")
    }

    fn dispatch_triggered_by(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        #[derive(Deserialize)]
        struct Args {
            text: String,
        }
        let a: Args = parse(args, "triggered_by")?;
        let reg = self.registry.lock().map_err(poisoned)?;
        let skills: Vec<_> = reg.triggered_by(&a.text).cloned().collect();
        to_value(&skills, "triggered_by")
    }

    fn dispatch_render(&self, args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
        #[derive(Deserialize)]
        struct Args {
            id: String,
            #[serde(default)]
            values: std::collections::HashMap<String, serde_json::Value>,
        }
        let a: Args = parse(args, "render")?;
        let reg = self.registry.lock().map_err(poisoned)?;
        let skill = reg
            .get(&a.id)
            .ok_or_else(|| exec_err(format!("no skill with id '{}'", a.id)))?;
        let values: std::collections::HashMap<String, serde_yaml::Value> = a
            .values
            .into_iter()
            .map(|(k, v)| {
                // Round-trip JSON → YAML so enum comparisons match the
                // skill's declared `values:` list shape.
                let y = serde_yaml::to_value(&v).unwrap_or(serde_yaml::Value::Null);
                (k, y)
            })
            .collect();
        let rendered =
            crate::render(skill, &values).map_err(|e| exec_err(format!("render: {e}")))?;
        Ok(serde_json::json!({
            "id": skill.meta.id,
            "name": skill.meta.name,
            "body": rendered,
        }))
    }

    /// BL-021 — resolve a skill's `depends_on` closure. Returns the
    /// ordered fragment list, a merged body string, and any non-fatal
    /// conflict warnings. Cycle / missing-dependency are surfaced as
    /// `ExecutionFailed` so the planner can fall back to the raw body.
    fn dispatch_compose(&self, args: &serde_json::Value) -> Result<serde_json::Value, PluginError> {
        #[derive(Deserialize)]
        struct Args {
            id: String,
        }
        let a: Args = parse(args, "compose")?;
        let reg = self.registry.lock().map_err(poisoned)?;
        match crate::compose::compose(&reg, &a.id) {
            Ok(composed) => to_value(&composed, "compose"),
            Err(err) => Err(exec_err(format!("compose: {err}"))),
        }
    }

    fn dispatch_reload(&self) -> Result<serde_json::Value, PluginError> {
        let reloaded = SkillRegistry::load(&self.root).unwrap_or_else(|err| {
            tracing::warn!(
                path = %self.root.display(),
                err = %err,
                "com.nexus.skills reload: partial or full failure; old registry replaced with parsed subset"
            );
            match err {
                SkillRegistryError::PartialParseFailure { .. } => {
                    // `load` returns the error AFTER populating the
                    // registry with the successfully-parsed subset,
                    // but discards it. Re-load once more into an
                    // empty registry so we at least keep whatever
                    // parsed cleanly.
                    SkillRegistry::load(&self.root).unwrap_or_else(|_| SkillRegistry::empty())
                }
                SkillRegistryError::Io(_) => SkillRegistry::empty(),
            }
        });
        let len = reloaded.len();
        // Best-effort: refresh the on-disk REGISTRY.json index so a
        // subsequent cold-start `load_with_index` reflects the new
        // walk. Failures log and do not abort the reload.
        let index_path = self.root.join("REGISTRY.json");
        if let Err(err) = registry_index::write_index(&index_path, &self.root, &reloaded) {
            tracing::warn!(
                path = %index_path.display(),
                err = %err,
                "com.nexus.skills reload: failed to refresh REGISTRY.json"
            );
        }
        *self.registry.lock().map_err(poisoned)? = reloaded;
        Ok(serde_json::json!({ "loaded": len }))
    }
}

// ── Error / serde plumbing ──────────────────────────────────────────────────

fn exec_err(reason: String) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason,
    }
}

fn poisoned<T>(_e: std::sync::PoisonError<T>) -> PluginError {
    exec_err("skills registry mutex poisoned — prior handler panicked".into())
}

fn parse<T: serde::de::DeserializeOwned>(
    args: &serde_json::Value,
    command: &str,
) -> Result<T, PluginError> {
    serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("{command}: invalid args: {e}")))
}

fn to_value<T: serde::Serialize>(v: &T, command: &str) -> Result<serde_json::Value, PluginError> {
    serde_json::to_value(v).map_err(|e| exec_err(format!("{command}: serialize: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const SKILL_A: &str = r#"---
name: A
id: skill-a
description: first
version: 1.0.0
author: me
created: 2026-04-01
tags: [alpha]
applicable_contexts: [ai-chat]
triggers: ["alpha mode"]
---
body A
"#;

    fn write_skill(dir: &std::path::Path, filename: &str, contents: &str) {
        std::fs::write(dir.join(filename), contents).unwrap();
    }

    #[test]
    fn list_round_trips_through_dispatch() {
        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "a.skill.md", SKILL_A);
        let mut plugin = SkillsCorePlugin::open(tmp.path().to_path_buf());
        let v = plugin
            .dispatch(HANDLER_LIST, &serde_json::json!({}))
            .unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "skill-a");
    }

    #[test]
    fn get_returns_error_for_unknown_id() {
        let tmp = TempDir::new().unwrap();
        let mut plugin = SkillsCorePlugin::open(tmp.path().to_path_buf());
        let err = plugin
            .dispatch(HANDLER_GET, &serde_json::json!({ "id": "missing" }))
            .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(reason.contains("no skill"));
            }
            _ => panic!("unexpected error"),
        }
    }

    #[test]
    fn list_by_context_filters_correctly() {
        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "a.skill.md", SKILL_A);
        let mut plugin = SkillsCorePlugin::open(tmp.path().to_path_buf());
        let v = plugin
            .dispatch(
                HANDLER_LIST_BY_CONTEXT,
                &serde_json::json!({ "context": "editor" }),
            )
            .unwrap();
        assert_eq!(v.as_array().unwrap().len(), 0);
        let v = plugin
            .dispatch(
                HANDLER_LIST_BY_CONTEXT,
                &serde_json::json!({ "context": "ai-chat" }),
            )
            .unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);
    }

    #[test]
    fn triggered_by_matches_case_insensitively() {
        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "a.skill.md", SKILL_A);
        let mut plugin = SkillsCorePlugin::open(tmp.path().to_path_buf());
        let v = plugin
            .dispatch(
                HANDLER_TRIGGERED_BY,
                &serde_json::json!({ "text": "please enter ALPHA MODE" }),
            )
            .unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);
    }

    #[test]
    fn render_substitutes_declared_parameters() {
        const SKILL_WITH_PARAM: &str = r"---
name: P
id: skill-p
description: d
version: 1.0.0
author: me
created: 2026-04-18
parameters:
  - name: tone
    type: string
    default: friendly
---
Write in a {{ tone }} style.
";
        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "p.skill.md", SKILL_WITH_PARAM);
        let mut plugin = SkillsCorePlugin::open(tmp.path().to_path_buf());
        let v = plugin
            .dispatch(
                HANDLER_RENDER,
                &serde_json::json!({ "id": "skill-p", "values": { "tone": "formal" } }),
            )
            .unwrap();
        assert_eq!(v["body"], "Write in a formal style.\n");

        let v = plugin
            .dispatch(HANDLER_RENDER, &serde_json::json!({ "id": "skill-p" }))
            .unwrap();
        assert_eq!(v["body"], "Write in a friendly style.\n");
    }

    #[test]
    fn render_errors_on_unknown_skill() {
        let tmp = TempDir::new().unwrap();
        let mut plugin = SkillsCorePlugin::open(tmp.path().to_path_buf());
        let err = plugin
            .dispatch(HANDLER_RENDER, &serde_json::json!({ "id": "missing" }))
            .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(reason.contains("no skill"));
            }
            _ => panic!("unexpected error"),
        }
    }

    #[test]
    fn open_writes_registry_json_after_load() {
        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "a.skill.md", SKILL_A);
        let _plugin = SkillsCorePlugin::open(tmp.path().to_path_buf());

        let index_path = tmp.path().join("REGISTRY.json");
        assert!(index_path.is_file(), "open must persist REGISTRY.json");
        let parsed = crate::registry_index::read_index(&index_path).unwrap();
        assert_eq!(parsed.skills.len(), 1);
        assert_eq!(parsed.skills[0].id, "skill-a");
    }

    #[test]
    fn reload_handler_rewrites_index() {
        const SKILL_B: &str = r"---
name: B
id: skill-b
description: second
version: 1.0.0
author: me
created: 2026-04-02
---
body B
";
        let tmp = TempDir::new().unwrap();
        write_skill(tmp.path(), "a.skill.md", SKILL_A);
        let mut plugin = SkillsCorePlugin::open(tmp.path().to_path_buf());

        // After open, the index lists exactly skill-a.
        let index_path = tmp.path().join("REGISTRY.json");
        let initial = crate::registry_index::read_index(&index_path).unwrap();
        assert_eq!(initial.skills.len(), 1);

        // Add a second skill on disk and trigger a reload.
        write_skill(tmp.path(), "b.skill.md", SKILL_B);
        let v = plugin
            .dispatch(HANDLER_RELOAD, &serde_json::json!({}))
            .unwrap();
        assert_eq!(v["loaded"], 2);

        let after = crate::registry_index::read_index(&index_path).unwrap();
        assert_eq!(after.skills.len(), 2);
        let ids: Vec<&str> = after.skills.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"skill-a"));
        assert!(ids.contains(&"skill-b"));
    }

    #[test]
    fn reload_picks_up_new_files() {
        let tmp = TempDir::new().unwrap();
        let mut plugin = SkillsCorePlugin::open(tmp.path().to_path_buf());
        assert_eq!(
            plugin
                .dispatch(HANDLER_LIST, &serde_json::json!({}))
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            0
        );
        write_skill(tmp.path(), "a.skill.md", SKILL_A);
        let v = plugin
            .dispatch(HANDLER_RELOAD, &serde_json::json!({}))
            .unwrap();
        assert_eq!(v["loaded"], 1);
    }
}
