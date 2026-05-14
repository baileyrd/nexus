//! BL-113 / ADR 0027 — Protocol-host adapter contribution aggregator.
//!
//! Host crates (`nexus-lsp`, `nexus-dap`, `nexus-mcp`, future
//! `nexus-acp`) call [`collect_contributions`] over the set of loaded
//! plugin manifests to obtain a tagged view of which plugin contributed
//! which adapter for each protocol family. The result is then merged
//! with the host's legacy flat-TOML config during the Phase 5
//! deprecation window — see ADR 0027 §Migration.
//!
//! Phase 0a (this module) is pure aggregation: no IPC, no side effects,
//! no host wiring. The host crates pick up the new shape in Phase 1+
//! per protocol.

use crate::manifest::{
    AcpProtocolHostReg, DapProtocolHostReg, LspProtocolHostReg, McpProtocolHostReg, PluginManifest,
};

/// One adapter contribution tagged with the plugin that supplied it.
///
/// `plugin_id` is the reverse-DNS identifier of the contributing plugin
/// (`PluginManifest::id`). Host crates use it for diagnostics ("LSP
/// adapter `rust-analyzer` contributed by `community.rust-lang`"), for
/// permission gating (a contribution from a disabled plugin should not
/// activate), and as a stable dedup key when the same adapter id is
/// contributed by multiple plugins.
#[derive(Debug, Clone)]
pub struct ContributedAdapter<T> {
    /// Reverse-DNS id of the contributing plugin.
    pub plugin_id: String,
    /// The adapter contribution itself, verbatim from the manifest.
    pub adapter: T,
}

/// Aggregated contributions across every loaded plugin, grouped by
/// protocol-host family. Each family vector preserves discovery order
/// (the iteration order of the input slice).
#[derive(Debug, Clone, Default)]
pub struct ContributedAdapterSet {
    /// LSP adapter contributions.
    pub lsp: Vec<ContributedAdapter<LspProtocolHostReg>>,
    /// DAP adapter contributions.
    pub dap: Vec<ContributedAdapter<DapProtocolHostReg>>,
    /// MCP server contributions.
    pub mcp: Vec<ContributedAdapter<McpProtocolHostReg>>,
    /// ACP agent contributions.
    pub acp: Vec<ContributedAdapter<AcpProtocolHostReg>>,
}

impl ContributedAdapterSet {
    /// `true` when no plugin in the input set contributed any adapter
    /// across any of the four protocol-host families. Hosts can short-
    /// circuit their merge path when this is `true` and keep operating
    /// on the legacy flat-TOML config alone.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lsp.is_empty() && self.dap.is_empty() && self.mcp.is_empty() && self.acp.is_empty()
    }

    /// Total contributed-adapter count across all four families.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lsp.len() + self.dap.len() + self.mcp.len() + self.acp.len()
    }
}

/// Aggregate every protocol-host contribution across a slice of loaded
/// plugin manifests. Manifests with no `[registrations.protocol_hosts]`
/// section contribute nothing; this is a no-op for them.
///
/// The result borrows nothing from the input — clones are cheap because
/// the contribution types are themselves shallow.
pub fn collect_contributions<'a, I>(manifests: I) -> ContributedAdapterSet
where
    I: IntoIterator<Item = &'a PluginManifest>,
{
    let mut out = ContributedAdapterSet::default();
    for m in manifests {
        let ph = &m.registrations.protocol_hosts;
        if ph.is_empty() {
            continue;
        }
        for a in &ph.lsp {
            out.lsp.push(ContributedAdapter {
                plugin_id: m.id.clone(),
                adapter: a.clone(),
            });
        }
        for a in &ph.dap {
            out.dap.push(ContributedAdapter {
                plugin_id: m.id.clone(),
                adapter: a.clone(),
            });
        }
        for a in &ph.mcp {
            out.mcp.push(ContributedAdapter {
                plugin_id: m.id.clone(),
                adapter: a.clone(),
            });
        }
        for a in &ph.acp {
            out.acp.push(ContributedAdapter {
                plugin_id: m.id.clone(),
                adapter: a.clone(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse_manifest;

    fn manifest_with_lsp(id: &str, lsp_id: &str) -> PluginManifest {
        let toml = format!(
            r#"
[plugin]
id = "{id}"
name = "Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.protocol_hosts.lsp]]
id = "{lsp_id}"
command = "x"
"#,
        );
        parse_manifest(&toml, "manifest.toml").unwrap()
    }

    fn manifest_with_no_contributions(id: &str) -> PluginManifest {
        let toml = format!(
            r#"
[plugin]
id = "{id}"
name = "Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"
"#,
        );
        parse_manifest(&toml, "manifest.toml").unwrap()
    }

    #[test]
    fn empty_input_yields_empty_set() {
        let out = collect_contributions::<&[_]>(&[]);
        assert!(out.is_empty());
        assert_eq!(out.len(), 0);
    }

    #[test]
    fn manifests_without_contributions_are_skipped() {
        let a = manifest_with_no_contributions("a");
        let b = manifest_with_no_contributions("b");
        let out = collect_contributions([&a, &b]);
        assert!(out.is_empty());
    }

    #[test]
    fn tags_each_adapter_with_its_plugin_id() {
        let a = manifest_with_lsp("community.alpha", "lsp.alpha");
        let b = manifest_with_lsp("community.beta", "lsp.beta");
        let out = collect_contributions([&a, &b]);
        assert_eq!(out.lsp.len(), 2);
        assert_eq!(out.lsp[0].plugin_id, "community.alpha");
        assert_eq!(out.lsp[0].adapter.id, "lsp.alpha");
        assert_eq!(out.lsp[1].plugin_id, "community.beta");
        assert_eq!(out.lsp[1].adapter.id, "lsp.beta");
    }

    #[test]
    fn aggregates_all_four_families() {
        let toml = r#"
[plugin]
id = "community.everything"
name = "Everything"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.protocol_hosts.lsp]]
id = "lsp.a"
command = "x"

[[registrations.protocol_hosts.dap]]
id = "dap.a"
command = "x"

[[registrations.protocol_hosts.mcp]]
id = "mcp.a"
command = "x"

[[registrations.protocol_hosts.acp]]
id = "acp.a"
command = "x"
"#;
        let m = parse_manifest(toml, "manifest.toml").unwrap();
        let out = collect_contributions([&m]);
        assert_eq!(out.len(), 4);
        assert_eq!(out.lsp[0].adapter.id, "lsp.a");
        assert_eq!(out.dap[0].adapter.id, "dap.a");
        assert_eq!(out.mcp[0].adapter.id, "mcp.a");
        assert_eq!(out.acp[0].adapter.id, "acp.a");
        assert!(
            out.lsp[0].plugin_id == "community.everything"
                && out.dap[0].plugin_id == "community.everything"
                && out.mcp[0].plugin_id == "community.everything"
                && out.acp[0].plugin_id == "community.everything",
        );
    }
}
