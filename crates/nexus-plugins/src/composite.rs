//! Composite [`IpcDispatcher`] for crossing the community / core plugin
//! registry boundary.
//!
//! Nexus intentionally keeps community plugins (in [`PluginManager`]) and
//! core plugins (in [`SharedPluginLoader`]) in separate registries — the
//! two have different trust levels, different lifecycles, and different
//! runtime backends. But a community WASM plugin that holds the
//! [`Capability::IpcCall`] capability still needs to be able to call
//! `ipc_call("com.nexus.storage", "read_file", …)` on a core plugin, or
//! the microkernel architecture falls over at exactly the boundary it
//! was built to serve.
//!
//! [`CompositeIpcDispatcher`] solves this without merging the two
//! registries: it delegates to a primary dispatcher (community) and,
//! if the primary returns [`IpcError::PluginNotFound`], retries through
//! an optional fallback (core). The fallback is write-once-after-init
//! because the core loader is built lazily (it needs the forge root).
//!
//! Only `PluginNotFound` triggers fall-through — `CommandNotFound`
//! (plugin exists in primary but doesn't expose this command) is
//! returned as-is, so a community plugin can't "shadow" a core command
//! by registering its own and have the call silently re-route.
//!
//! [`PluginManager`]: crate::PluginManager
//! [`SharedPluginLoader`]: crate::SharedPluginLoader
//! [`Capability::IpcCall`]: nexus_kernel::Capability::IpcCall

use std::sync::{Arc, Mutex};

use nexus_kernel::{IpcDispatcher, IpcError, IpcFuture};

/// Write-once cell holding the core-plugin fallback dispatcher.
///
/// Cloneable; all clones share the same interior. Start as empty via
/// [`FallbackCell::new`] and install the loader once it's built via
/// [`FallbackCell::set`].
#[derive(Clone, Default)]
pub struct FallbackCell(Arc<Mutex<Option<Arc<dyn IpcDispatcher>>>>);

impl FallbackCell {
    /// Construct an empty cell.
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }

    /// Install the fallback dispatcher. Overwrites any previous value.
    pub fn set(&self, dispatcher: Arc<dyn IpcDispatcher>) {
        if let Ok(mut guard) = self.0.lock() {
            *guard = Some(dispatcher);
        }
    }

    /// Snapshot the current fallback, if any.
    #[must_use]
    pub fn get(&self) -> Option<Arc<dyn IpcDispatcher>> {
        self.0.lock().ok()?.as_ref().map(Arc::clone)
    }
}

/// [`IpcDispatcher`] that tries a primary dispatcher, then a fallback.
///
/// Only falls through on [`IpcError::PluginNotFound`]. Commands resolved
/// by the primary — even when they fail — are returned as-is so the
/// caller sees the real error.
pub struct CompositeIpcDispatcher {
    primary: Arc<dyn IpcDispatcher>,
    fallback: FallbackCell,
}

impl CompositeIpcDispatcher {
    /// Build a composite with the given primary and fallback cell.
    #[must_use]
    pub fn new(primary: Arc<dyn IpcDispatcher>, fallback: FallbackCell) -> Self {
        Self { primary, fallback }
    }
}

impl IpcDispatcher for CompositeIpcDispatcher {
    fn dispatch(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, IpcError> {
        match self.primary.dispatch(target_plugin_id, command_id, args) {
            Err(IpcError::PluginNotFound { plugin_id }) => {
                if let Some(fb) = self.fallback.get() {
                    fb.dispatch(target_plugin_id, command_id, args)
                } else {
                    Err(IpcError::PluginNotFound { plugin_id })
                }
            }
            other => other,
        }
    }

    fn dispatch_async(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        args: serde_json::Value,
    ) -> Option<IpcFuture> {
        if let Some(fut) = self
            .primary
            .dispatch_async(target_plugin_id, command_id, args.clone())
        {
            return Some(fut);
        }
        self.fallback
            .get()
            .and_then(|fb| fb.dispatch_async(target_plugin_id, command_id, args))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedDispatcher(Result<serde_json::Value, IpcError>);

    impl IpcDispatcher for FixedDispatcher {
        fn dispatch(
            &self,
            _t: &str,
            _c: &str,
            _a: &serde_json::Value,
        ) -> Result<serde_json::Value, IpcError> {
            self.0.clone()
        }
    }

    fn ok(value: &str) -> Arc<dyn IpcDispatcher> {
        Arc::new(FixedDispatcher(Ok(serde_json::json!({ "from": value }))))
    }

    fn not_found(plugin_id: &str) -> Arc<dyn IpcDispatcher> {
        Arc::new(FixedDispatcher(Err(IpcError::PluginNotFound {
            plugin_id: plugin_id.to_string(),
        })))
    }

    fn command_not_found(plugin_id: &str, command: &str) -> Arc<dyn IpcDispatcher> {
        Arc::new(FixedDispatcher(Err(IpcError::CommandNotFound {
            plugin_id: plugin_id.to_string(),
            command: command.to_string(),
        })))
    }

    #[test]
    fn primary_success_short_circuits() {
        let cell = FallbackCell::new();
        cell.set(ok("fallback"));
        let composite = CompositeIpcDispatcher::new(ok("primary"), cell);

        let out = composite
            .dispatch("com.x", "do", &serde_json::json!({}))
            .unwrap();

        assert_eq!(out, serde_json::json!({ "from": "primary" }));
    }

    #[test]
    fn primary_plugin_not_found_falls_through() {
        let cell = FallbackCell::new();
        cell.set(ok("fallback"));
        let composite = CompositeIpcDispatcher::new(not_found("com.x"), cell);

        let out = composite
            .dispatch("com.x", "do", &serde_json::json!({}))
            .unwrap();

        assert_eq!(out, serde_json::json!({ "from": "fallback" }));
    }

    #[test]
    fn command_not_found_does_not_fall_through() {
        let cell = FallbackCell::new();
        cell.set(ok("fallback"));
        let composite = CompositeIpcDispatcher::new(command_not_found("com.x", "nope"), cell);

        let err = composite
            .dispatch("com.x", "nope", &serde_json::json!({}))
            .unwrap_err();

        assert!(matches!(err, IpcError::CommandNotFound { .. }));
    }

    #[test]
    fn empty_fallback_returns_primary_not_found() {
        let cell = FallbackCell::new();
        let composite = CompositeIpcDispatcher::new(not_found("com.x"), cell);

        let err = composite
            .dispatch("com.x", "do", &serde_json::json!({}))
            .unwrap_err();

        assert!(matches!(err, IpcError::PluginNotFound { .. }));
    }

    #[test]
    fn fallback_installed_after_construction_is_used() {
        let cell = FallbackCell::new();
        let composite = CompositeIpcDispatcher::new(not_found("com.x"), cell.clone());

        // Before installation: propagates the primary's PluginNotFound.
        assert!(composite
            .dispatch("com.x", "do", &serde_json::json!({}))
            .is_err());

        // After installation: falls through.
        cell.set(ok("late"));
        let out = composite
            .dispatch("com.x", "do", &serde_json::json!({}))
            .unwrap();
        assert_eq!(out, serde_json::json!({ "from": "late" }));
    }
}
