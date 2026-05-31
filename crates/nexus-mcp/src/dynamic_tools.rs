//! DG-39 / PRD-14 §10 — in-process dynamic MCP tool registry.
//!
//! The `tool_router`-generated `nexus_*` tool surface is static at
//! compile time. To let other plugins (core or community) publish
//! their own MCP tools at runtime — PRD-14 §10's "Plugin Command →
//! MCP Tool Flow" — we layer a thread-safe registry alongside the
//! static router. The MCP server's `list_tools` returns the union of
//! static + dynamic; `call_tool` checks the dynamic registry first
//! and routes hits back through `ipc_call(plugin_id, command, args)`.
//!
//! The registry is process-global (same pattern as
//! `nexus_kernel::audit_store`) because the IPC handlers in
//! `McpHostPlugin::dispatch` and the `NexusMcpServer` instance built
//! by `nexus mcp serve` need to share state without threading an Arc
//! through bootstrap.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock, PoisonError, RwLock};

use serde::{Deserialize, Serialize};

/// One dynamic tool declaration. Carries the metadata the MCP server
/// surfaces to clients (`name`, `description`, `input_schema`) plus
/// the kernel-side IPC target (`plugin_id`, `command`) that
/// `call_tool` routes invocations to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicTool {
    /// Publicly-visible tool name. Must not collide with a name in
    /// the static `nexus_*` set (enforced at registration time —
    /// only the `nexus_` prefix is reserved; everything else is the
    /// publishing plugin's namespace to police).
    pub name: String,
    /// Human-readable description shown to MCP clients.
    pub description: String,
    /// JSON Schema for the tool's input parameters. Empty object
    /// (`{"type":"object","properties":{}}`) is acceptable for
    /// no-argument tools.
    pub input_schema: serde_json::Value,
    /// Reverse-DNS id of the publishing plugin (the `ipc_call`
    /// target).
    pub plugin_id: String,
    /// IPC command name on that plugin (the second argument to
    /// `KernelPluginContext::ipc_call`).
    pub command: String,
}

/// Errors from [`DynamicToolRegistry`] operations.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// A tool with the given name is already registered. Plugins
    /// must unregister before re-registering under the same name.
    #[error("tool '{0}' is already registered")]
    DuplicateName(String),
    /// The tool name uses the reserved `nexus_` prefix.
    #[error(
        "tool name '{0}' is reserved (the 'nexus_' prefix is owned by the static MCP tool set)"
    )]
    ReservedPrefix(String),
    /// The tool name is empty or otherwise invalid.
    #[error("tool name '{0}' is invalid: {1}")]
    InvalidName(String, &'static str),
}

/// In-process registry. The interior `RwLock` is uncontended in the
/// common path (one writer at register/unregister time, many readers
/// when `list_tools` / `call_tool` resolve).
#[derive(Debug, Default)]
pub struct DynamicToolRegistry {
    inner: RwLock<BTreeMap<String, DynamicTool>>,
}

impl DynamicToolRegistry {
    /// Empty registry. Use [`global`] in production code; this
    /// constructor is for tests that want isolated registries.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new tool. Fails on duplicate name, reserved
    /// prefix, or empty name.
    ///
    /// # Errors
    /// See [`RegistryError`].
    pub fn register(&self, tool: DynamicTool) -> Result<(), RegistryError> {
        validate_name(&tool.name)?;
        let mut guard = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        if guard.contains_key(&tool.name) {
            return Err(RegistryError::DuplicateName(tool.name));
        }
        guard.insert(tool.name.clone(), tool);
        Ok(())
    }

    /// Remove a tool by name. Returns `true` if it was present.
    pub fn unregister(&self, name: &str) -> bool {
        let mut guard = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        guard.remove(name).is_some()
    }

    /// Look up a tool by name.
    ///
    /// Cloned so the lock is released before the caller drives an
    /// async IPC call.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<DynamicTool> {
        let guard = self.inner.read().unwrap_or_else(PoisonError::into_inner);
        guard.get(name).cloned()
    }

    /// Snapshot every registered tool, sorted by name (`BTreeMap`
    /// iteration order). Cloned for the same lock-hygiene reason
    /// as [`Self::lookup`].
    #[must_use]
    pub fn list(&self) -> Vec<DynamicTool> {
        let guard = self.inner.read().unwrap_or_else(PoisonError::into_inner);
        guard.values().cloned().collect()
    }

    /// Number of currently-registered tools. Mostly useful for
    /// tests / metrics.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner
            .read()
            .map_or_else(|e| e.into_inner().len(), |g| g.len())
    }

    /// True iff no tool is currently registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn validate_name(name: &str) -> Result<(), RegistryError> {
    if name.is_empty() {
        return Err(RegistryError::InvalidName(
            name.to_string(),
            "must not be empty",
        ));
    }
    if name.starts_with("nexus_") {
        return Err(RegistryError::ReservedPrefix(name.to_string()));
    }
    Ok(())
}

// ── Global accessor ─────────────────────────────────────────────────────────

static GLOBAL: OnceLock<Arc<DynamicToolRegistry>> = OnceLock::new();

/// Process-wide registry. Lazily initialised on first access.
#[must_use]
pub fn global() -> Arc<DynamicToolRegistry> {
    Arc::clone(GLOBAL.get_or_init(|| Arc::new(DynamicToolRegistry::new())))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample(name: &str) -> DynamicTool {
        DynamicTool {
            name: name.to_string(),
            description: format!("desc for {name}"),
            input_schema: json!({ "type": "object", "properties": {} }),
            plugin_id: "com.example.plugin".to_string(),
            command: "do_thing".to_string(),
        }
    }

    #[test]
    fn register_and_lookup() {
        let reg = DynamicToolRegistry::new();
        assert!(reg.is_empty());
        reg.register(sample("plugin_thing")).unwrap();
        assert_eq!(reg.len(), 1);
        let got = reg.lookup("plugin_thing").unwrap();
        assert_eq!(got.plugin_id, "com.example.plugin");
        assert_eq!(got.command, "do_thing");
    }

    #[test]
    fn register_duplicate_name_is_rejected() {
        let reg = DynamicToolRegistry::new();
        reg.register(sample("plugin_thing")).unwrap();
        let err = reg.register(sample("plugin_thing")).unwrap_err();
        assert!(matches!(err, RegistryError::DuplicateName(_)));
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn register_reserved_prefix_is_rejected() {
        let reg = DynamicToolRegistry::new();
        let err = reg.register(sample("nexus_read_note")).unwrap_err();
        assert!(matches!(err, RegistryError::ReservedPrefix(_)));
        assert!(reg.is_empty());
    }

    #[test]
    fn register_empty_name_is_rejected() {
        let reg = DynamicToolRegistry::new();
        let err = reg.register(sample("")).unwrap_err();
        assert!(matches!(err, RegistryError::InvalidName(_, _)));
    }

    #[test]
    fn unregister_returns_false_for_missing() {
        let reg = DynamicToolRegistry::new();
        assert!(!reg.unregister("never_registered"));
    }

    #[test]
    fn unregister_then_reregister_succeeds() {
        let reg = DynamicToolRegistry::new();
        reg.register(sample("plugin_thing")).unwrap();
        assert!(reg.unregister("plugin_thing"));
        assert!(reg.is_empty());
        // Re-registration after unregister must succeed — that's the
        // contract that lets plugins hot-reload tool metadata.
        reg.register(sample("plugin_thing")).unwrap();
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn list_returns_alphabetical_snapshot() {
        let reg = DynamicToolRegistry::new();
        reg.register(sample("zeta")).unwrap();
        reg.register(sample("alpha")).unwrap();
        reg.register(sample("mu")).unwrap();
        let names: Vec<_> = reg.list().into_iter().map(|t| t.name).collect();
        assert_eq!(names, vec!["alpha", "mu", "zeta"]);
    }

    #[test]
    fn global_returns_same_instance() {
        // Pointer identity: two `global()` calls return Arcs to the
        // same registry, so handler-side mutations are visible to
        // server-side reads.
        let a = global();
        let b = global();
        assert!(Arc::ptr_eq(&a, &b));
    }
}
