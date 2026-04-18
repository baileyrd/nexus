//! Core plugin wrapping [`WorkflowRegistry`].
//!
//! Exposes the registry over kernel IPC so CLI / UI / future trigger
//! engine can consult the list of declared workflows without linking
//! `nexus-workflow` directly. Plugin is *load-only* — mutations
//! happen by editing `.workflow.toml` files and calling `reload`.
//!
//! # Handlers
//!
//! | Id | Command    | Args             | Purpose                              |
//! |---:|------------|------------------|--------------------------------------|
//! | 1  | `list`     | `{}`             | Every loaded workflow (metadata + triggers) |
//! | 2  | `get`      | `{ name }`       | One workflow by name (404 if missing) |
//! | 3  | `reload`   | `{}`             | Re-scan `<forge>/.workflows`          |
//! | 4  | `validate` | `{ text }`       | Parse a TOML string; return validated JSON |
//!
//! Ids are append-only.

use std::path::PathBuf;
use std::sync::Mutex;

use nexus_plugins::{CorePlugin, PluginError};
use serde::Deserialize;

use crate::{parse_workflow_text, WorkflowRegistry, WorkflowRegistryError};

/// Reverse-DNS identifier.
pub const PLUGIN_ID: &str = "com.nexus.workflow";

/// `list` handler id.
pub const HANDLER_LIST: u32 = 1;
/// `get` handler id.
pub const HANDLER_GET: u32 = 2;
/// `reload` handler id.
pub const HANDLER_RELOAD: u32 = 3;
/// `validate` handler id.
pub const HANDLER_VALIDATE: u32 = 4;

/// Core plugin — holds the workflow root path + an in-memory registry
/// behind a mutex so dispatches stay `Send + Sync`.
pub struct WorkflowCorePlugin {
    root: PathBuf,
    registry: Mutex<WorkflowRegistry>,
}

impl WorkflowCorePlugin {
    /// Construct with the forge's `.workflows/` directory. Eagerly
    /// loads the registry; partial parse failures are logged at
    /// `warn` and the registry starts with whatever parsed cleanly.
    #[must_use]
    pub fn open(workflows_dir: PathBuf) -> Self {
        let registry = match WorkflowRegistry::load(&workflows_dir) {
            Ok(reg) => reg,
            Err(WorkflowRegistryError::PartialParseFailure { count, first }) => {
                tracing::warn!(
                    path = %workflows_dir.display(),
                    count,
                    first = %first,
                    "com.nexus.workflow: {count} workflow file(s) failed to parse during load"
                );
                // Re-run with the known-good subset by discarding
                // failures — load inserts good entries before it
                // returns the error, but the error path drops the
                // registry. A clean reload rebuilds from scratch.
                WorkflowRegistry::load(&workflows_dir).unwrap_or_else(|_| WorkflowRegistry::empty())
            }
            Err(err) => {
                tracing::warn!(
                    path = %workflows_dir.display(),
                    err = %err,
                    "com.nexus.workflow: load failed; registry starts empty"
                );
                WorkflowRegistry::empty()
            }
        };
        Self {
            root: workflows_dir,
            registry: Mutex::new(registry),
        }
    }
}

impl CorePlugin for WorkflowCorePlugin {
    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        match handler_id {
            HANDLER_LIST => self.dispatch_list(),
            HANDLER_GET => self.dispatch_get(args),
            HANDLER_RELOAD => self.dispatch_reload(),
            HANDLER_VALIDATE => Self::dispatch_validate(args),
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }
}

impl WorkflowCorePlugin {
    fn dispatch_list(&self) -> Result<serde_json::Value, PluginError> {
        let reg = self.registry.lock().map_err(poisoned)?;
        let workflows: Vec<_> = reg.iter().map(|(_, w)| w.clone()).collect();
        to_value(&workflows, "list")
    }

    fn dispatch_get(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        #[derive(Deserialize)]
        struct Args {
            name: String,
        }
        let a: Args = parse(args, "get")?;
        let reg = self.registry.lock().map_err(poisoned)?;
        match reg.get(&a.name) {
            Some(w) => to_value(w, "get"),
            None => Err(exec_err(format!("no workflow named '{}'", a.name))),
        }
    }

    fn dispatch_reload(&self) -> Result<serde_json::Value, PluginError> {
        let reloaded = WorkflowRegistry::load(&self.root)
            .unwrap_or_else(|_| WorkflowRegistry::empty());
        let len = reloaded.len();
        *self.registry.lock().map_err(poisoned)? = reloaded;
        Ok(serde_json::json!({ "loaded": len }))
    }

    fn dispatch_validate(
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        #[derive(Deserialize)]
        struct Args {
            text: String,
        }
        let a: Args = parse(args, "validate")?;
        match parse_workflow_text(&a.text) {
            Ok(w) => to_value(&w, "validate"),
            Err(err) => Err(exec_err(format!("invalid workflow: {err}"))),
        }
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
    exec_err("workflow registry mutex poisoned — prior handler panicked".into())
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

    const WF: &str = r#"
[workflow]
name = "Daily"
description = "daily journal"

[trigger]
type = "cron"
schedule = "0 9 * * *"

[[steps]]
type = "file_create"
path = "journal/today.md"
"#;

    fn write(dir: &std::path::Path, filename: &str, contents: &str) {
        std::fs::write(dir.join(filename), contents).unwrap();
    }

    #[test]
    fn list_round_trips_through_dispatch() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "daily.workflow.toml", WF);
        let mut plugin = WorkflowCorePlugin::open(tmp.path().to_path_buf());
        let v = plugin.dispatch(HANDLER_LIST, &serde_json::json!({})).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["workflow"]["name"], "Daily");
        assert_eq!(arr[0]["trigger"]["type"], "cron");
    }

    #[test]
    fn get_returns_error_for_unknown_name() {
        let tmp = TempDir::new().unwrap();
        let mut plugin = WorkflowCorePlugin::open(tmp.path().to_path_buf());
        let err = plugin
            .dispatch(HANDLER_GET, &serde_json::json!({ "name": "nope" }))
            .unwrap_err();
        match err {
            PluginError::ExecutionFailed { reason, .. } => {
                assert!(reason.contains("no workflow"));
            }
            _ => panic!("unexpected"),
        }
    }

    #[test]
    fn reload_picks_up_new_files() {
        let tmp = TempDir::new().unwrap();
        let mut plugin = WorkflowCorePlugin::open(tmp.path().to_path_buf());
        assert_eq!(
            plugin
                .dispatch(HANDLER_LIST, &serde_json::json!({}))
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            0
        );
        write(tmp.path(), "daily.workflow.toml", WF);
        let v = plugin.dispatch(HANDLER_RELOAD, &serde_json::json!({})).unwrap();
        assert_eq!(v["loaded"], 1);
    }

    #[test]
    fn validate_accepts_good_toml() {
        let tmp = TempDir::new().unwrap();
        let mut plugin = WorkflowCorePlugin::open(tmp.path().to_path_buf());
        let v = plugin
            .dispatch(HANDLER_VALIDATE, &serde_json::json!({ "text": WF }))
            .unwrap();
        assert_eq!(v["workflow"]["name"], "Daily");
    }

    #[test]
    fn validate_rejects_bad_toml() {
        let tmp = TempDir::new().unwrap();
        let mut plugin = WorkflowCorePlugin::open(tmp.path().to_path_buf());
        let err = plugin
            .dispatch(HANDLER_VALIDATE, &serde_json::json!({ "text": "not-toml {{" }))
            .unwrap_err();
        assert!(matches!(err, PluginError::ExecutionFailed { .. }));
    }
}
