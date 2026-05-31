//! BL-113 Phase 4 / BL-144 — integration tests for the bootstrap-side
//! `wire_acp_contributions` / `unwire_acp_contributions_for_plugin`
//! pair against a real booted runtime.
//!
//! The ACP host plugin is registered eagerly at bootstrap (see
//! `register_core_plugins` in `crates/nexus-bootstrap/src/lib.rs`),
//! so `MinimalForge::new()` is enough — no extra setup beyond
//! synthesising community-plugin manifests that declare ACP
//! contributions.

#![cfg(not(target_arch = "wasm32"))]

#[path = "common/mod.rs"]
mod common;

use common::MinimalForge;
use nexus_bootstrap::acp_contribution_wiring::{
    unwire_acp_contributions_for_plugin, wire_acp_contributions, AcpWireStatus,
};
use nexus_plugins::{parse_manifest, PluginManifest};

fn manifest_with_two_acp_agents() -> PluginManifest {
    let toml = r#"
[plugin]
id = "community.hermes-pack"
name = "Hermes Agent Pack"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.protocol_hosts.acp]]
id = "hermes"
display_name = "Hermes"
command = "hermes-agent"
args = ["--stdio"]
capabilities = ["delegate", "tools"]

[[registrations.protocol_hosts.acp]]
id = "hermes-coder"
display_name = "Hermes Coder"
command = "hermes-agent"
args = ["--coder"]
capabilities = ["delegate", "tools", "code"]
"#;
    parse_manifest(toml, "hermes-pack.manifest.toml").unwrap()
}

fn manifest_with_invalid_acp_agent() -> PluginManifest {
    // Manifest parsing accepts the entry; the host's
    // `register_contributed` is what rejects empty command.
    let toml = r#"
[plugin]
id = "community.broken-acp"
name = "Broken ACP"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.protocol_hosts.acp]]
id = "broken"
command = "   "
"#;
    parse_manifest(toml, "broken-acp.manifest.toml").unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn wire_acp_contributions_registers_every_contribution() {
    let forge = MinimalForge::new();
    let manifest = manifest_with_two_acp_agents();
    let outcomes = wire_acp_contributions(&forge.runtime.context, &[&manifest]).await;
    assert_eq!(outcomes.len(), 2);
    for o in &outcomes {
        assert_eq!(o.status, AcpWireStatus::Ok, "outcome was {o:?}");
        assert_eq!(o.plugin_id, "community.hermes-pack");
    }
    // The host exposes the merged set via list_agents — verify both
    // contributions landed and carry the metadata we packed in
    // protocol_host_specs::acp_contribution_to_spec.
    let reply = forge
        .ipc_call("com.nexus.acp", "list_agents", serde_json::json!({}))
        .await
        .unwrap();
    let entries = reply.as_array().unwrap();
    assert_eq!(entries.len(), 2);
    let names: Vec<String> = entries
        .iter()
        .map(|v| v["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"hermes".to_string()));
    assert!(names.contains(&"hermes-coder".to_string()));
    let hermes = entries.iter().find(|e| e["name"] == "hermes").unwrap();
    assert_eq!(
        hermes["capabilities"],
        serde_json::json!(["delegate", "tools"])
    );
    assert_eq!(hermes["metadata"]["plugin_id"], "community.hermes-pack");
    assert_eq!(hermes["metadata"]["display_name"], "Hermes");
}

#[tokio::test(flavor = "current_thread")]
async fn wire_acp_contributions_reports_invalid_command_as_skip_status() {
    let forge = MinimalForge::new();
    let manifest = manifest_with_invalid_acp_agent();
    let outcomes = wire_acp_contributions(&forge.runtime.context, &[&manifest]).await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].status, AcpWireStatus::InvalidCommand);
    let reply = forge
        .ipc_call("com.nexus.acp", "list_agents", serde_json::json!({}))
        .await
        .unwrap();
    assert!(reply.as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn wire_then_unwire_is_idempotent_round_trip() {
    let forge = MinimalForge::new();
    let manifest = manifest_with_two_acp_agents();
    let wired = wire_acp_contributions(&forge.runtime.context, &[&manifest]).await;
    assert!(wired.iter().all(|o| o.status == AcpWireStatus::Ok));
    let unwired = unwire_acp_contributions_for_plugin(&forge.runtime.context, &manifest).await;
    assert_eq!(unwired.len(), 2);
    for o in &unwired {
        assert_eq!(o.status, AcpWireStatus::Ok, "outcome was {o:?}");
    }
    let reply = forge
        .ipc_call("com.nexus.acp", "list_agents", serde_json::json!({}))
        .await
        .unwrap();
    assert!(reply.as_array().unwrap().is_empty());

    // A second unwire returns not_found for every entry — no panic,
    // no dispatch error.
    let second = unwire_acp_contributions_for_plugin(&forge.runtime.context, &manifest).await;
    assert_eq!(second.len(), 2);
    for o in &second {
        assert_eq!(o.status, AcpWireStatus::NotFound);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn unwire_refuses_when_called_by_different_plugin() {
    let forge = MinimalForge::new();
    let manifest_a = manifest_with_two_acp_agents();
    wire_acp_contributions(&forge.runtime.context, &[&manifest_a]).await;

    let manifest_b = parse_manifest(
        r#"
[plugin]
id = "community.intruder"
name = "Intruder"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.protocol_hosts.acp]]
id = "hermes"
command = "doesnt-matter"

[[registrations.protocol_hosts.acp]]
id = "hermes-coder"
command = "doesnt-matter"
"#,
        "intruder.manifest.toml",
    )
    .unwrap();
    let outcomes = unwire_acp_contributions_for_plugin(&forge.runtime.context, &manifest_b).await;
    assert_eq!(outcomes.len(), 2);
    for o in &outcomes {
        match &o.status {
            AcpWireStatus::NotOwnedByPlugin { actual_owner } => {
                assert_eq!(actual_owner, "community.hermes-pack");
            }
            other => panic!("expected NotOwnedByPlugin, got {other:?}"),
        }
    }

    // A's agents survive the failed intruder attempt.
    let reply = forge
        .ipc_call("com.nexus.acp", "list_agents", serde_json::json!({}))
        .await
        .unwrap();
    assert_eq!(reply.as_array().unwrap().len(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn wire_handles_empty_manifest_set_without_dispatching() {
    let forge = MinimalForge::new();
    let outcomes = wire_acp_contributions(&forge.runtime.context, &[]).await;
    assert!(outcomes.is_empty());
}

/// BL-144 — the in-tree reference plugin at
/// `plugins/first-party-acp-echo/manifest.toml` parses, wires through
/// `register_server`, and surfaces on `list_agents` with the shell-only
/// fields packed into `metadata` so a future agent picker can read
/// them without an extra IPC.
#[tokio::test(flavor = "current_thread")]
async fn first_party_acp_echo_plugin_wires_and_surfaces_metadata() {
    let manifest_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("plugins")
        .join("first-party-acp-echo")
        .join("manifest.toml");
    let toml = std::fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", manifest_path.display()));
    let manifest = parse_manifest(&toml, manifest_path.to_str().unwrap()).unwrap();
    assert_eq!(manifest.id, "first-party.acp.echo");
    assert_eq!(manifest.registrations.protocol_hosts.acp.len(), 1);
    let acp = &manifest.registrations.protocol_hosts.acp[0];
    assert_eq!(acp.id, "echo");

    let forge = MinimalForge::new();
    let outcomes = wire_acp_contributions(&forge.runtime.context, &[&manifest]).await;
    assert_eq!(outcomes.len(), 1);
    assert!(
        matches!(outcomes[0].status, AcpWireStatus::Ok),
        "first-party.acp.echo should wire cleanly, got {:?}",
        outcomes[0].status,
    );

    let reply = forge
        .ipc_call("com.nexus.acp", "list_agents", serde_json::json!({}))
        .await
        .unwrap();
    let echo = reply
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "echo")
        .expect("echo agent should appear in list_agents")
        .clone();
    assert_eq!(echo["metadata"]["plugin_id"], "first-party.acp.echo");
    assert!(echo["metadata"]["display_name"].is_string());
}
