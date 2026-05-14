//! BL-113 / ADR 0027 — bridge between manifest-side
//! `nexus_plugins::ContributedAdapter<{Lsp,Mcp}ProtocolHostReg>` and
//! the host-side `{Lsp,Mcp}ServerSpec` shapes.
//!
//! This module is the only place in the tree that does that mapping
//! so the host crates don't have to depend on `nexus-plugins` and the
//! manifest types don't have to know about per-host spec shapes.
//!
//! Phase 2a/3a (this module) is the conversion layer. Phase 2b/3b will
//! wire the result through `LspHostConfig::merge_contributed` /
//! `McpHostConfig::merge_contributed` after the plugin scan completes
//! — needs a plugin-lifecycle callback design that's deliberately
//! held until Phase 1 (DAP) lands and we know what the callback shape
//! looks like in practice.

use nexus_lsp::LspServerSpec;
use nexus_mcp::{McpServerSpec, McpTransport};
use nexus_plugins::{
    ContributedAdapter, ContributedAdapterSet, LspProtocolHostReg, McpProtocolHostReg,
};

/// Convert a single LSP contribution into the pair shape
/// `LspHostConfig::merge_contributed` expects: `(spec, plugin_id)`.
///
/// `LspProtocolHostReg.env` is a `BTreeMap` (matches the manifest
/// TOML's ordered table); `LspServerSpec.env` is a `HashMap` (matches
/// the host's existing flat-TOML schema). The conversion is a
/// pair-by-pair drain.
#[must_use]
pub fn lsp_contribution_to_spec(
    contribution: ContributedAdapter<LspProtocolHostReg>,
) -> (LspServerSpec, String) {
    let ContributedAdapter { plugin_id, adapter } = contribution;
    let spec = LspServerSpec {
        // ADR 0027 keeps the manifest's `id` as the stable identifier
        // and lets `display_name` be a separate UI string. The LSP
        // host's existing `name` field plays the role of "stable id"
        // — they map to the same concept on this side of the bridge.
        name: adapter.id,
        command: adapter.command,
        args: adapter.args,
        file_types: adapter.file_types,
        root_markers: adapter.root_markers,
        disabled: adapter.disabled,
        env: adapter.env.into_iter().collect(),
    };
    (spec, plugin_id)
}

/// Convert every LSP contribution in `set` into the merge-ready list.
/// Preserves contribution order so the host's `MergeSkip` list is
/// stable across calls.
#[must_use]
pub fn lsp_contributions_to_specs(set: &ContributedAdapterSet) -> Vec<(LspServerSpec, String)> {
    set.lsp
        .iter()
        .cloned()
        .map(lsp_contribution_to_spec)
        .collect()
}

/// Convert a single MCP contribution into the triple shape
/// `McpHostConfig::merge_contributed` expects: `(name, spec,
/// plugin_id)`. MCP uses a `BTreeMap<String, McpServerSpec>` keyed on
/// the server name, so the name is hoisted out of the spec at the
/// call boundary.
///
/// `McpProtocolHostReg.transport` is a free-form string in the
/// manifest (so a community plugin can spell a future transport
/// without a manifest schema bump). We parse it here against the
/// closed `McpTransport` enum; unknown values fall back to
/// `Stdio` since that's the historical default + matches the
/// manifest TOML default helper.
#[must_use]
pub fn mcp_contribution_to_spec(
    contribution: ContributedAdapter<McpProtocolHostReg>,
) -> (String, McpServerSpec, String) {
    let ContributedAdapter { plugin_id, adapter } = contribution;
    let transport = parse_mcp_transport(&adapter.transport);
    let spec = McpServerSpec {
        transport,
        command: adapter.command.unwrap_or_default(),
        args: adapter.args,
        env: adapter.env,
        url: adapter.url,
        auth_header: None,
        headers: Default::default(),
        auth: None,
        disabled: adapter.disabled,
    };
    (adapter.id, spec, plugin_id)
}

/// Convert every MCP contribution in `set`. Preserves order.
#[must_use]
pub fn mcp_contributions_to_specs(
    set: &ContributedAdapterSet,
) -> Vec<(String, McpServerSpec, String)> {
    set.mcp
        .iter()
        .cloned()
        .map(mcp_contribution_to_spec)
        .collect()
}

fn parse_mcp_transport(raw: &str) -> McpTransport {
    match raw.trim().to_ascii_lowercase().as_str() {
        "http" => McpTransport::Http,
        "ws" | "websocket" => McpTransport::Websocket,
        _ => McpTransport::Stdio,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_plugins::parse_manifest;

    fn collect_for(toml: &str) -> ContributedAdapterSet {
        let m = parse_manifest(toml, "manifest.toml").unwrap();
        nexus_plugins::collect_contributions([&m])
    }

    #[test]
    fn lsp_conversion_round_trips_every_field() {
        let toml = r#"
[plugin]
id = "community.rust"
name = "Rust Pack"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.protocol_hosts.lsp]]
id = "rust-analyzer"
command = "rust-analyzer"
args = ["--log", "info"]
file_types = ["rs"]
root_markers = ["Cargo.toml"]
disabled = true
env = { RUST_LOG = "trace" }
"#;
        let set = collect_for(toml);
        let specs = lsp_contributions_to_specs(&set);
        assert_eq!(specs.len(), 1);
        let (spec, plugin_id) = &specs[0];
        assert_eq!(plugin_id, "community.rust");
        assert_eq!(spec.name, "rust-analyzer");
        assert_eq!(spec.command, "rust-analyzer");
        assert_eq!(spec.args, ["--log", "info"]);
        assert_eq!(spec.file_types, ["rs"]);
        assert_eq!(spec.root_markers, ["Cargo.toml"]);
        assert!(spec.disabled);
        assert_eq!(spec.env.get("RUST_LOG").map(String::as_str), Some("trace"));
    }

    #[test]
    fn mcp_conversion_defaults_transport_to_stdio() {
        let toml = r#"
[plugin]
id = "community.mcp"
name = "MCP Pack"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.protocol_hosts.mcp]]
id = "fs"
command = "filesystem-mcp"
args = ["--root", "."]
"#;
        let set = collect_for(toml);
        let specs = mcp_contributions_to_specs(&set);
        assert_eq!(specs.len(), 1);
        let (name, spec, plugin_id) = &specs[0];
        assert_eq!(name, "fs");
        assert_eq!(plugin_id, "community.mcp");
        assert_eq!(spec.transport, McpTransport::Stdio);
        assert_eq!(spec.command, "filesystem-mcp");
        assert_eq!(spec.args, ["--root", "."]);
    }

    #[test]
    fn mcp_conversion_parses_remote_transports() {
        let toml = r#"
[plugin]
id = "community.remote"
name = "Remote MCPs"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.protocol_hosts.mcp]]
id = "alpha"
transport = "http"
url = "https://alpha.example.com/mcp"

[[registrations.protocol_hosts.mcp]]
id = "beta"
transport = "websocket"
url = "wss://beta.example.com/mcp"

[[registrations.protocol_hosts.mcp]]
id = "ws-alias"
transport = "ws"
url = "wss://ws.example.com/mcp"

[[registrations.protocol_hosts.mcp]]
id = "unknown-falls-back"
transport = "tcp"
command = "x"
"#;
        let set = collect_for(toml);
        let specs = mcp_contributions_to_specs(&set);
        assert_eq!(specs.len(), 4);
        assert_eq!(specs[0].1.transport, McpTransport::Http);
        assert_eq!(specs[1].1.transport, McpTransport::Websocket);
        assert_eq!(specs[2].1.transport, McpTransport::Websocket);
        // Unknown transport falls back to stdio (the manifest default).
        assert_eq!(specs[3].1.transport, McpTransport::Stdio);
    }

    #[test]
    fn empty_contribution_set_yields_empty_specs() {
        let set = ContributedAdapterSet::default();
        assert!(lsp_contributions_to_specs(&set).is_empty());
        assert!(mcp_contributions_to_specs(&set).is_empty());
    }
}
