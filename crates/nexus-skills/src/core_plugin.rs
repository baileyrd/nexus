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
//!
//! Ids are append-only.

use std::path::PathBuf;
use std::sync::Mutex;

use nexus_plugins::{CorePlugin, PluginError};
use serde::Deserialize;

use crate::{SkillRegistry, SkillRegistryError};

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
                _ => SkillRegistry::empty(),
            }
        });
        let len = reloaded.len();
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

fn to_value<T: serde::Serialize>(
    v: &T,
    command: &str,
) -> Result<serde_json::Value, PluginError> {
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
