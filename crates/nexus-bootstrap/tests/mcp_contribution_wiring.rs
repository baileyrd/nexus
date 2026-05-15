//! BL-113 Phase 3b — integration tests for the bootstrap-side
//! `wire_mcp_contributions` / `unwire_mcp_contributions_for_plugin`
//! pair against a real booted runtime.

#![cfg(not(target_arch = "wasm32"))]

#[path = "common/mod.rs"]
mod common;

use common::MinimalForge;
use nexus_bootstrap::mcp_contribution_wiring::{
    unwire_mcp_contributions_for_plugin, wire_mcp_contributions, McpWireStatus,
};
use nexus_plugins::{parse_manifest, PluginManifest};

fn manifest_with_two_mcp_servers() -> PluginManifest {
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

[[registrations.protocol_hosts.mcp]]
id = "github"
command = "github-mcp"
"#;
    parse_manifest(toml, "mcp.manifest.toml").unwrap()
}

fn manifest_with_invalid_mcp_server() -> PluginManifest {
    // Empty command on stdio transport — host validator rejects.
    let toml = r#"
[plugin]
id = "community.broken"
name = "Broken"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.protocol_hosts.mcp]]
id = "broken"
"#;
    parse_manifest(toml, "broken.manifest.toml").unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn wire_mcp_contributions_registers_every_contribution() {
    let forge = MinimalForge::new();
    let manifest = manifest_with_two_mcp_servers();
    let outcomes = wire_mcp_contributions(&forge.runtime.context, &[&manifest]).await;
    assert_eq!(outcomes.len(), 2);
    for o in &outcomes {
        assert_eq!(o.status, McpWireStatus::Ok, "outcome was {o:?}");
        assert_eq!(o.plugin_id, "community.mcp");
    }
    let reply = forge
        .ipc_call("com.nexus.mcp.host", "list_servers", serde_json::json!({}))
        .await
        .unwrap();
    let names: Vec<String> = reply
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"fs".to_string()));
    assert!(names.contains(&"github".to_string()));
}

#[tokio::test(flavor = "current_thread")]
async fn wire_mcp_contributions_reports_invalid_as_skip_status() {
    let forge = MinimalForge::new();
    let manifest = manifest_with_invalid_mcp_server();
    let outcomes = wire_mcp_contributions(&forge.runtime.context, &[&manifest]).await;
    assert_eq!(outcomes.len(), 1);
    assert!(matches!(outcomes[0].status, McpWireStatus::Invalid(_)));
    let reply = forge
        .ipc_call("com.nexus.mcp.host", "list_servers", serde_json::json!({}))
        .await
        .unwrap();
    assert!(reply.as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn wire_then_unwire_is_idempotent_round_trip() {
    let forge = MinimalForge::new();
    let manifest = manifest_with_two_mcp_servers();

    let wired = wire_mcp_contributions(&forge.runtime.context, &[&manifest]).await;
    assert!(wired.iter().all(|o| o.status == McpWireStatus::Ok));

    let unwired = unwire_mcp_contributions_for_plugin(&forge.runtime.context, &manifest).await;
    assert_eq!(unwired.len(), 2);
    for o in &unwired {
        assert_eq!(o.status, McpWireStatus::Ok, "outcome was {o:?}");
    }

    let reply = forge
        .ipc_call("com.nexus.mcp.host", "list_servers", serde_json::json!({}))
        .await
        .unwrap();
    assert!(reply.as_array().unwrap().is_empty());

    let second = unwire_mcp_contributions_for_plugin(&forge.runtime.context, &manifest).await;
    assert_eq!(second.len(), 2);
    for o in &second {
        assert_eq!(o.status, McpWireStatus::NotFound);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn unwire_refuses_when_called_by_different_plugin() {
    let forge = MinimalForge::new();
    let manifest_a = manifest_with_two_mcp_servers();
    wire_mcp_contributions(&forge.runtime.context, &[&manifest_a]).await;

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

[[registrations.protocol_hosts.mcp]]
id = "fs"
command = "doesnt-matter"

[[registrations.protocol_hosts.mcp]]
id = "github"
command = "doesnt-matter"
"#,
        "intruder.manifest.toml",
    )
    .unwrap();
    let outcomes = unwire_mcp_contributions_for_plugin(&forge.runtime.context, &manifest_b).await;
    assert_eq!(outcomes.len(), 2);
    for o in &outcomes {
        match &o.status {
            McpWireStatus::NotOwnedByPlugin { actual_owner } => {
                assert_eq!(actual_owner, "community.mcp");
            }
            other => panic!("expected NotOwnedByPlugin, got {other:?}"),
        }
    }

    let reply = forge
        .ipc_call("com.nexus.mcp.host", "list_servers", serde_json::json!({}))
        .await
        .unwrap();
    let names: Vec<String> = reply
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"fs".to_string()));
    assert!(names.contains(&"github".to_string()));
}

#[tokio::test(flavor = "current_thread")]
async fn wire_handles_empty_manifest_set_without_dispatching() {
    let forge = MinimalForge::new();
    let outcomes = wire_mcp_contributions(&forge.runtime.context, &[]).await;
    assert!(outcomes.is_empty());
}
