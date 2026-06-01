//! BL-060 ad-hoc command history handlers — `adhoc_list`,
//! `adhoc_get`, `adhoc_delete`, `adhoc_promote`. Split out of
//! `core_plugin.rs` by SD-03 terminal chunk 1.

use crate::core_plugin::TerminalCorePlugin;
use crate::ipc::{AdHocIdArgs, AdHocListArgs, AdHocPromoteArgs};
use crate::saved::{promote_adhoc_to_saved, PromoteOptions};
use nexus_plugins::PluginError;

use super::shared::{crate_err, parse_args, poisoned, to_value};

impl TerminalCorePlugin {
    /// `adhoc_list` — most-recent-first slice of the ad-hoc history.
    ///
    /// `limit` defaults to 100 to keep payloads bounded; callers that
    /// genuinely want everything can request a larger value but should
    /// expect O(rows) cost. Wrapped here rather than in the store so
    /// the store's plain-library shape stays unchanged.
    pub(crate) fn dispatch_adhoc_list(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: AdHocListArgs = parse_args(args, "adhoc_list")?;
        let limit = usize::try_from(a.limit.unwrap_or(100)).unwrap_or(usize::MAX);
        let store = self.adhoc_store()?.lock().map_err(poisoned)?;
        let rows = store.recent(limit).map_err(crate_err)?;
        to_value(&rows, "adhoc_list")
    }

    /// `adhoc_get` — single row by id, or JSON `null` when unknown.
    pub(crate) fn dispatch_adhoc_get(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: AdHocIdArgs = parse_args(args, "adhoc_get")?;
        let store = self.adhoc_store()?.lock().map_err(poisoned)?;
        let row = store.get(&a.id).map_err(crate_err)?;
        match row {
            Some(r) => to_value(&r, "adhoc_get"),
            None => Ok(serde_json::Value::Null),
        }
    }

    /// `adhoc_delete` — idempotent. Returns `{ id }` regardless of
    /// whether the row existed; the store's `DELETE` is a no-op on a
    /// missing id and the caller already knows the id they passed.
    pub(crate) fn dispatch_adhoc_delete(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: AdHocIdArgs = parse_args(args, "adhoc_delete")?;
        let store = self.adhoc_store()?.lock().map_err(poisoned)?;
        store.delete(&a.id).map_err(crate_err)?;
        Ok(serde_json::json!({ "id": a.id }))
    }

    /// `adhoc_promote` — wraps [`promote_adhoc_to_saved`]. Requires
    /// both the ad-hoc and saved stores to be attached.
    pub(crate) fn dispatch_adhoc_promote(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: AdHocPromoteArgs = parse_args(args, "adhoc_promote")?;
        // Lock the adhoc store for the read, then drop before locking
        // saved — keeps lock-acquisition order one-way and avoids any
        // cross-call deadlock with a future handler that takes both.
        let adhoc_lock = self.adhoc_store()?.lock().map_err(poisoned)?;
        let saved_lock = self.saved_store()?.lock().map_err(poisoned)?;
        let opts = PromoteOptions {
            slug: a.slug,
            icon: a.icon,
            shell: a.shell,
        };
        let cmd = promote_adhoc_to_saved(&adhoc_lock, &saved_lock, &a.id, a.name, opts)
            .map_err(crate_err)?;
        to_value(&cmd, "adhoc_promote")
    }
}
