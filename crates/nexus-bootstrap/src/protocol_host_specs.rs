//! BL-113 / ADR 0027 — bridge between manifest-side
//! `nexus_plugins::ContributedAdapter<{Lsp,Dap,Mcp}ProtocolHostReg>`
//! and the host-side `{Lsp,Dap,Mcp}` spec shapes.
//!
//! This module is the only place in the tree that does that mapping
//! so the host crates don't have to depend on `nexus-plugins` and the
//! manifest types don't have to know about per-host spec shapes.
//!
//! Phase 1a/2a/3a (this module) is the conversion layer. Phase 1b/2b/3b
//! will wire the result through `DapHostConfig::merge_contributed` /
//! `LspHostConfig::merge_contributed` / `McpHostConfig::merge_contributed`
//! after the plugin scan completes — needs a plugin-lifecycle callback
//! design that's tracked under BL-113.

use nexus_dap::DapAdapterSpec;
use nexus_lsp::LspServerSpec;
use nexus_mcp::{McpServerSpec, McpTransport};
use nexus_plugins::{
    ContributedAdapter, ContributedAdapterSet, DapProtocolHostReg, LspProtocolHostReg,
    McpProtocolHostReg,
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

/// Convert a single DAP contribution into the pair shape
/// `DapHostConfig::merge_contributed` expects: `(spec, plugin_id)`.
///
/// `DapProtocolHostReg.env` is a `BTreeMap` (matches the manifest
/// TOML's ordered table); `DapAdapterSpec.env` is a `HashMap` (matches
/// the host's existing flat-TOML schema). The conversion is a
/// pair-by-pair drain.
///
/// `display_name`, `root_markers`, `launch_config_schema`, and
/// `variable_renderers` don't affect host behaviour, so they don't
/// land in the typed spec fields — but the shell needs them to render
/// a typed launch-config form, badge the adapter in the picker, and
/// pick variable formatters. They ride along as opaque `metadata` on
/// the spec instead: the host stores the JSON verbatim and surfaces it
/// on `list_adapters` so the shell can read it without an extra IPC.
///
/// `launch_config_schema` is passed through **as the relative path
/// string** declared in the manifest, not as inline schema content —
/// resolving it would require reading the plugin's filesystem from a
/// pure manifest-transform function. The shell resolves the path
/// against the plugin directory it already knows from
/// `scan_plugin_directory_at` (which the Tauri bridge serves).
///
/// `metadata` is `Some({...})` whenever at least one shell-only field
/// is non-empty; otherwise `None` so TOML-loaded specs and barebones
/// contributions look identical on the wire.
#[must_use]
pub fn dap_contribution_to_spec(
    contribution: ContributedAdapter<DapProtocolHostReg>,
) -> (DapAdapterSpec, String) {
    let ContributedAdapter { plugin_id, adapter } = contribution;
    let metadata = build_dap_contribution_metadata(&plugin_id, &adapter);
    let spec = DapAdapterSpec {
        // ADR 0027 keeps the manifest's `id` as the stable identifier;
        // the host's `name` field plays the same role on this side.
        name: adapter.id,
        command: adapter.command,
        args: adapter.args,
        // The manifest has no `type` field — the cosmetic adapter-type
        // hint is dap.toml-only and stays `None` for contributions.
        adapter_type: None,
        file_types: adapter.file_types,
        disabled: adapter.disabled,
        env: adapter.env.into_iter().collect(),
        metadata,
    };
    (spec, plugin_id)
}

/// Pack the manifest's shell-only fields into the opaque `metadata`
/// JSON the host round-trips on `list_adapters`. Returns `None` when
/// every shell-only field is empty so contributions that don't care
/// about a launch-config form look identical to TOML entries on the
/// wire.
fn build_dap_contribution_metadata(
    plugin_id: &str,
    adapter: &DapProtocolHostReg,
) -> Option<serde_json::Value> {
    let has_payload = adapter.display_name.is_some()
        || adapter.launch_config_schema.is_some()
        || !adapter.root_markers.is_empty()
        || !adapter.variable_renderers.is_empty();
    if !has_payload {
        return None;
    }
    let mut obj = serde_json::Map::new();
    obj.insert(
        "plugin_id".to_string(),
        serde_json::Value::String(plugin_id.to_string()),
    );
    if let Some(name) = &adapter.display_name {
        obj.insert(
            "display_name".to_string(),
            serde_json::Value::String(name.clone()),
        );
    }
    if let Some(path) = &adapter.launch_config_schema {
        // Relative-path-as-string; shell resolves against the plugin dir.
        obj.insert(
            "launch_config_schema".to_string(),
            serde_json::Value::String(path.clone()),
        );
    }
    if !adapter.root_markers.is_empty() {
        obj.insert(
            "root_markers".to_string(),
            serde_json::Value::Array(
                adapter
                    .root_markers
                    .iter()
                    .cloned()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
    }
    if !adapter.variable_renderers.is_empty() {
        obj.insert(
            "variable_renderers".to_string(),
            serde_json::Value::Array(
                adapter
                    .variable_renderers
                    .iter()
                    .cloned()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
    }
    Some(serde_json::Value::Object(obj))
}

/// Convert every DAP contribution in `set` into the merge-ready list.
/// Preserves contribution order so the host's skip list is stable
/// across calls.
#[must_use]
pub fn dap_contributions_to_specs(set: &ContributedAdapterSet) -> Vec<(DapAdapterSpec, String)> {
    set.dap
        .iter()
        .cloned()
        .map(dap_contribution_to_spec)
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
        assert!(dap_contributions_to_specs(&set).is_empty());
        assert!(mcp_contributions_to_specs(&set).is_empty());
    }

    #[test]
    fn dap_conversion_round_trips_every_field() {
        let toml = r#"
[plugin]
id = "community.rust-debug"
name = "Rust Debugger"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.protocol_hosts.dap]]
id = "rust"
display_name = "Rust (codelldb)"
command = "codelldb"
args = ["--port", "0"]
file_types = ["rs"]
root_markers = ["Cargo.toml"]
launch_config_schema = "./launch.schema.json"
variable_renderers = ["rust_vec", "rust_option"]
disabled = true
env = { RUST_BACKTRACE = "1" }
"#;
        let set = collect_for(toml);
        let specs = dap_contributions_to_specs(&set);
        assert_eq!(specs.len(), 1);
        let (spec, plugin_id) = &specs[0];
        assert_eq!(plugin_id, "community.rust-debug");
        assert_eq!(spec.name, "rust");
        assert_eq!(spec.command, "codelldb");
        assert_eq!(spec.args, ["--port", "0"]);
        assert_eq!(spec.file_types, ["rs"]);
        assert!(spec.disabled);
        // adapter_type is None for contributions — dap.toml-only field.
        assert!(spec.adapter_type.is_none());
        assert_eq!(
            spec.env.get("RUST_BACKTRACE").map(String::as_str),
            Some("1"),
        );
        // BL-113 — shell-only fields ride through opaque `metadata`.
        let md = spec
            .metadata
            .as_ref()
            .expect("contribution with display_name + schema + markers + renderers should set metadata");
        assert_eq!(md["plugin_id"], "community.rust-debug");
        assert_eq!(md["display_name"], "Rust (codelldb)");
        assert_eq!(md["launch_config_schema"], "./launch.schema.json");
        assert_eq!(md["root_markers"], serde_json::json!(["Cargo.toml"]));
        assert_eq!(
            md["variable_renderers"],
            serde_json::json!(["rust_vec", "rust_option"]),
        );
    }

    #[test]
    fn dap_conversion_omits_metadata_when_all_shell_fields_empty() {
        let toml = r#"
[plugin]
id = "community.bare-dap"
name = "Bare DAP"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.protocol_hosts.dap]]
id = "bare"
command = "bare-adapter"
"#;
        let set = collect_for(toml);
        let specs = dap_contributions_to_specs(&set);
        assert_eq!(specs.len(), 1);
        // No display_name, no launch_config_schema, no root_markers, no
        // variable_renderers → metadata stays None so the wire shape is
        // indistinguishable from a TOML-loaded entry.
        assert!(specs[0].0.metadata.is_none());
    }

    #[test]
    fn dap_conversion_includes_metadata_when_only_one_shell_field_set() {
        let toml = r#"
[plugin]
id = "community.partial-dap"
name = "Partial DAP"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.protocol_hosts.dap]]
id = "only-schema"
command = "x"
launch_config_schema = "./launch.schema.json"
"#;
        let set = collect_for(toml);
        let specs = dap_contributions_to_specs(&set);
        let md = specs[0]
            .0
            .metadata
            .as_ref()
            .expect("schema-only contribution still emits metadata");
        assert_eq!(md["plugin_id"], "community.partial-dap");
        assert_eq!(md["launch_config_schema"], "./launch.schema.json");
        // Fields that weren't set in the manifest are absent (not null).
        assert!(md.get("display_name").is_none());
        assert!(md.get("root_markers").is_none());
        assert!(md.get("variable_renderers").is_none());
    }

    #[test]
    fn dap_conversion_preserves_input_order() {
        let toml = r#"
[plugin]
id = "community.multi-dap"
name = "Multi DAP"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.protocol_hosts.dap]]
id = "alpha"
command = "a"

[[registrations.protocol_hosts.dap]]
id = "beta"
command = "b"

[[registrations.protocol_hosts.dap]]
id = "gamma"
command = "c"
"#;
        let set = collect_for(toml);
        let specs = dap_contributions_to_specs(&set);
        assert_eq!(specs.len(), 3);
        assert_eq!(specs[0].0.name, "alpha");
        assert_eq!(specs[1].0.name, "beta");
        assert_eq!(specs[2].0.name, "gamma");
    }
}
