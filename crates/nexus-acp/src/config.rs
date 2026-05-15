//! ACP adapter registry.
//!
//! Unlike `nexus-lsp` / `nexus-mcp` / `nexus-dap`, **there is no
//! `acp.toml`** — ADR 0027 §Phase 4 lands ACP greenfield under the
//! contribution model. All adapters arrive through
//! [`AcpHostConfig::register_contributed`] (called by the
//! `com.nexus.acp::register_server` IPC verb, which is in turn
//! dispatched by `nexus-bootstrap::acp_contribution_wiring`).
//!
//! The on-disk forge layout under `.forge/` therefore never owns an
//! ACP config file; an operator who needs a non-plugin adapter can
//! ship a minimal manifest-only plugin (no wasm) the same way the
//! first-party DAP example plugin does (`plugins/first-party-dap-python/`).
//!
//! The `contributed_by` map is the authorisation key for
//! [`AcpHostConfig::unregister_contributed`]: a plugin can only
//! unregister its own adapters.

use std::collections::HashMap;

use thiserror::Error;

/// Errors raised by the (non-existent today) flat-TOML loader. Kept
/// around so the public surface mirrors [`crate::config::AcpConfigError`]'s
/// peers in the other protocol-host crates — if a future operator
/// scenario forces a flat-TOML escape hatch, the error shape is
/// already in place.
#[derive(Debug, Error)]
pub enum AcpConfigError {
    /// A contribution had an empty required field after trimming.
    #[error("adapter entry missing required field '{field}'")]
    MissingField {
        /// Field that was empty / absent.
        field: &'static str,
    },
}

/// One configured ACP adapter. Mirrors
/// `nexus_plugins::manifest::AcpProtocolHostReg` but lives in the host
/// crate so consumers (the IPC handlers, the connection pool) don't
/// depend on `nexus-plugins`.
#[derive(Debug, Clone)]
pub struct AcpAdapterSpec {
    /// Stable identifier (manifest `id`). Used by the IPC handlers as
    /// the routing key and by [`AcpHostConfig::contributed_by`] as
    /// the authorisation key for `unregister_server`.
    pub name: String,
    /// Executable to spawn — looked up on `$PATH` if not absolute.
    pub command: String,
    /// CLI args appended to `command`.
    pub args: Vec<String>,
    /// Declarative capability tags advertised by the contributing
    /// manifest. Surfaced verbatim through `list_agents` so the shell
    /// can render an agent picker badged by capability. Today the
    /// host does not gate behaviour on this set; runtime authorisation
    /// rides on the kernel capability matrix attached to the
    /// contributing plugin.
    pub capabilities: Vec<String>,
    /// Set `true` to keep the entry registered but skip spawning. A
    /// future shell affordance can toggle this without re-installing
    /// the contributing plugin.
    pub disabled: bool,
    /// Environment merged on top of the host process's environment at
    /// spawn time. ACP adapters routinely need `ANTHROPIC_API_KEY` or
    /// equivalent, set through the contributing plugin's UX rather
    /// than the host's.
    pub env: HashMap<String, String>,
    /// Opaque shell-side metadata packed by
    /// `nexus-bootstrap::protocol_host_specs::acp_contribution_to_spec`
    /// at contribution time. Same pattern DAP uses for
    /// `launch_config_schema` / `variable_renderers` — the host stores
    /// the JSON verbatim and round-trips it through `list_agents` so
    /// the shell can read the contributing plugin's `display_name`,
    /// reverse-DNS `plugin_id`, and any future shell-only fields
    /// without an extra IPC round-trip.
    pub metadata: Option<serde_json::Value>,
}

/// In-memory adapter registry + runtime contribution state.
#[derive(Debug, Clone, Default)]
pub struct AcpHostConfig {
    /// Adapters keyed by [`AcpAdapterSpec::name`] for O(1) lookup.
    pub adapters: HashMap<String, AcpAdapterSpec>,
    /// BL-113 Phase 4 — maps adapter `name` to the contributing
    /// plugin's reverse-DNS id. Every entry in [`adapters`] also
    /// appears here (ACP has no TOML-loaded entries). Kept symmetric
    /// with the LSP / DAP / MCP host shape so future hardening can
    /// gate the IPC verbs on `caller_plugin_id == contributed_by`.
    ///
    /// [`adapters`]: Self::adapters
    pub contributed_by: HashMap<String, String>,
}

impl AcpHostConfig {
    /// Empty config — the only sane initial state for an ACP host
    /// (contributions are bolted on at plugin-load time).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// BL-113 / ADR 0027 — single-spec contribution registration. The
    /// `com.nexus.acp::register_server` IPC verb dispatches here.
    ///
    /// Validates `spec.name` / `spec.command` (empty after trim →
    /// rejected) and refuses a name already in use by any prior
    /// contribution. Same precedence rules as the other protocol
    /// hosts; ACP just doesn't have a TOML class.
    ///
    /// # Errors
    /// Returns a [`MergeSkipReason`] when the spec fails validation or
    /// collides with an existing entry. On `Err`, the config is
    /// unchanged.
    pub fn register_contributed(
        &mut self,
        spec: AcpAdapterSpec,
        plugin_id: String,
    ) -> Result<(), MergeSkipReason> {
        if spec.name.trim().is_empty() {
            return Err(MergeSkipReason::InvalidName);
        }
        if spec.command.trim().is_empty() {
            return Err(MergeSkipReason::InvalidCommand);
        }
        if self.adapters.contains_key(&spec.name) {
            return Err(MergeSkipReason::AlreadyRegistered);
        }
        let name = spec.name.clone();
        self.adapters.insert(name.clone(), spec);
        self.contributed_by.insert(name, plugin_id);
        Ok(())
    }

    /// Batch variant — register every contribution in `contributions`
    /// against the same precedence rules and collect per-spec skip
    /// reasons. Returns an empty vec on full success.
    pub fn merge_contributed(
        &mut self,
        contributions: Vec<(AcpAdapterSpec, String)>,
    ) -> Vec<MergeSkip> {
        let mut skipped = Vec::new();
        for (spec, plugin_id) in contributions {
            if let Err(reason) = self.register_contributed(spec.clone(), plugin_id.clone())
            {
                skipped.push(MergeSkip {
                    name: spec.name,
                    plugin_id,
                    reason,
                });
            }
        }
        skipped
    }

    /// BL-113 — remove a previously-contributed adapter. The
    /// `com.nexus.acp::unregister_server` IPC verb's entry point.
    ///
    /// `plugin_id` must match the contributing plugin recorded at
    /// register time. Plugins can't unregister adapters they don't
    /// own.
    ///
    /// # Errors
    /// - [`UnregisterError::NotFound`] when no adapter exists for `name`.
    /// - [`UnregisterError::NotOwnedByPlugin`] when the row exists but
    ///   `plugin_id` doesn't match the contributor.
    pub fn unregister_contributed(
        &mut self,
        name: &str,
        plugin_id: &str,
    ) -> Result<AcpAdapterSpec, UnregisterError> {
        match self.contributed_by.get(name) {
            None => Err(UnregisterError::NotFound),
            Some(owner) if owner != plugin_id => Err(UnregisterError::NotOwnedByPlugin {
                actual_owner: owner.clone(),
            }),
            Some(_) => {
                self.contributed_by.remove(name);
                self.adapters.remove(name).ok_or(UnregisterError::NotFound)
            }
        }
    }
}

/// Why a single contribution was dropped during
/// [`AcpHostConfig::merge_contributed`] / `register_contributed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeSkip {
    /// The contribution's `name` (may be empty when
    /// [`MergeSkipReason::InvalidName`]).
    pub name: String,
    /// Reverse-DNS id of the contributing plugin.
    pub plugin_id: String,
    /// Reason the contribution was not accepted.
    pub reason: MergeSkipReason,
}

/// Per-contribution skip reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeSkipReason {
    /// Another contribution already owns this `name`. Renamed from
    /// `TomlOverride` (LSP/DAP/MCP) since ACP has no TOML class.
    AlreadyRegistered,
    /// `name` was empty / whitespace-only.
    InvalidName,
    /// `command` was empty / whitespace-only.
    InvalidCommand,
}

/// Why [`AcpHostConfig::unregister_contributed`] refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnregisterError {
    /// No adapter exists under that name.
    NotFound,
    /// The adapter exists but was contributed by a different plugin.
    NotOwnedByPlugin {
        /// Reverse-DNS id of the plugin that actually owns the entry.
        actual_owner: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str, command: &str) -> AcpAdapterSpec {
        AcpAdapterSpec {
            name: name.to_string(),
            command: command.to_string(),
            args: vec![],
            capabilities: vec![],
            disabled: false,
            env: HashMap::new(),
            metadata: None,
        }
    }

    #[test]
    fn register_contributed_happy_path() {
        let mut cfg = AcpHostConfig::new();
        assert!(cfg
            .register_contributed(spec("hermes", "hermes-agent"), "community.hermes".into())
            .is_ok());
        assert_eq!(cfg.adapters["hermes"].command, "hermes-agent");
        assert_eq!(cfg.contributed_by["hermes"], "community.hermes");
    }

    #[test]
    fn register_contributed_rejects_invalid() {
        let mut cfg = AcpHostConfig::new();
        assert_eq!(
            cfg.register_contributed(spec("", "x"), "p".into())
                .unwrap_err(),
            MergeSkipReason::InvalidName,
        );
        assert_eq!(
            cfg.register_contributed(spec("ok", "  "), "p".into())
                .unwrap_err(),
            MergeSkipReason::InvalidCommand,
        );
        assert!(cfg.adapters.is_empty());
    }

    #[test]
    fn register_contributed_refuses_duplicate_name() {
        let mut cfg = AcpHostConfig::new();
        cfg.register_contributed(spec("a", "x"), "p1".into()).unwrap();
        assert_eq!(
            cfg.register_contributed(spec("a", "y"), "p2".into())
                .unwrap_err(),
            MergeSkipReason::AlreadyRegistered,
        );
        // First contribution untouched.
        assert_eq!(cfg.adapters["a"].command, "x");
        assert_eq!(cfg.contributed_by["a"], "p1");
    }

    #[test]
    fn merge_contributed_preserves_input_order_in_skipped() {
        let mut cfg = AcpHostConfig::new();
        cfg.register_contributed(spec("a", "x"), "p0".into()).unwrap();
        let skipped = cfg.merge_contributed(vec![
            (spec("a", "y"), "p1".into()),
            (spec("b", "ok"), "p2".into()),
            (spec("", "bad"), "p3".into()),
            (spec("a", "y2"), "p4".into()),
        ]);
        assert_eq!(skipped.len(), 3);
        assert_eq!(skipped[0].plugin_id, "p1");
        assert_eq!(skipped[1].plugin_id, "p3");
        assert_eq!(skipped[2].plugin_id, "p4");
        assert_eq!(cfg.adapters.len(), 2);
        assert!(cfg.adapters.contains_key("b"));
    }

    #[test]
    fn unregister_contributed_round_trip() {
        let mut cfg = AcpHostConfig::new();
        cfg.register_contributed(spec("a", "x"), "p1".into()).unwrap();
        let removed = cfg.unregister_contributed("a", "p1").unwrap();
        assert_eq!(removed.command, "x");
        assert!(cfg.adapters.is_empty());
        assert!(cfg.contributed_by.is_empty());
    }

    #[test]
    fn unregister_contributed_refuses_other_plugin() {
        let mut cfg = AcpHostConfig::new();
        cfg.register_contributed(spec("a", "x"), "owner".into()).unwrap();
        match cfg.unregister_contributed("a", "intruder") {
            Err(UnregisterError::NotOwnedByPlugin { actual_owner }) => {
                assert_eq!(actual_owner, "owner");
            }
            other => panic!("expected NotOwnedByPlugin, got {other:?}"),
        }
        assert!(cfg.adapters.contains_key("a"));
    }

    #[test]
    fn unregister_contributed_not_found() {
        let mut cfg = AcpHostConfig::new();
        assert_eq!(
            cfg.unregister_contributed("ghost", "any").unwrap_err(),
            UnregisterError::NotFound,
        );
    }
}
