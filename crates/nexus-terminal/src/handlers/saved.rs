//! Saved-command handlers — `saved_list`, `saved_create`,
//! `saved_update`, `saved_delete`, `saved_reorder`. Split out of
//! `core_plugin.rs` by SD-03 terminal chunk 1.

use crate::core_plugin::TerminalCorePlugin;
use crate::saved::SavedCommand;
use nexus_plugins::PluginError;

use super::shared::{crate_err, exec_err, parse_args, poisoned, to_value};

impl TerminalCorePlugin {
    pub(crate) fn dispatch_saved_list(&self) -> Result<serde_json::Value, PluginError> {
        let store = self.saved_store()?.lock().map_err(poisoned)?;
        let rows = store.list().map_err(crate_err)?;
        to_value(&rows, "saved_list")
    }

    pub(crate) fn dispatch_saved_create(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let cmd: SavedCommand = parse_args(args, "saved_create")?;
        let store = self.saved_store()?.lock().map_err(poisoned)?;
        store.create(&cmd).map_err(crate_err)?;
        to_value(&cmd, "saved_create")
    }

    pub(crate) fn dispatch_saved_update(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let cmd: SavedCommand = parse_args(args, "saved_update")?;
        let store = self.saved_store()?.lock().map_err(poisoned)?;
        store.update(&cmd).map_err(crate_err)?;
        let fresh = store
            .get(&cmd.slug)
            .map_err(crate_err)?
            .ok_or_else(|| exec_err(format!("saved_update: slug '{}' vanished", cmd.slug)))?;
        to_value(&fresh, "saved_update")
    }

    pub(crate) fn dispatch_saved_delete(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        #[derive(serde::Deserialize)]
        struct DeleteArgs {
            slug: String,
        }
        let a: DeleteArgs = parse_args(args, "saved_delete")?;
        let store = self.saved_store()?.lock().map_err(poisoned)?;
        store.delete(&a.slug).map_err(crate_err)?;
        Ok(serde_json::json!({ "slug": a.slug }))
    }

    pub(crate) fn dispatch_saved_reorder(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        #[derive(serde::Deserialize)]
        struct ReorderArgs {
            slug: String,
            #[serde(default)]
            sidebar_order: Option<i32>,
        }
        let a: ReorderArgs = parse_args(args, "saved_reorder")?;
        let store = self.saved_store()?.lock().map_err(poisoned)?;
        store.reorder(&a.slug, a.sidebar_order).map_err(crate_err)?;
        Ok(serde_json::json!({ "slug": a.slug, "sidebar_order": a.sidebar_order }))
    }
}
