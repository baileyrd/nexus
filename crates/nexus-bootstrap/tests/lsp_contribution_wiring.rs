//! BL-113 Phase 2b — integration tests for the bootstrap-side
//! `wire_lsp_contributions` / `unwire_lsp_contributions_for_plugin`
//! pair against a real booted runtime.

#![cfg(not(target_arch = "wasm32"))]

#[path = "common/mod.rs"]
mod common;

use common::MinimalForge;
use nexus_bootstrap::lsp_contribution_wiring::{
    unwire_lsp_contributions_for_plugin, wire_lsp_contributions, LspWireStatus,
};
use nexus_plugins::{parse_manifest, PluginManifest};

fn manifest_with_two_lsp_servers() -> PluginManifest {
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

[[registrations.protocol_hosts.lsp]]
id = "alt-server"
command = "alt-server"
file_types = ["rs"]
"#;
    parse_manifest(toml, "rust.manifest.toml").unwrap()
}

fn manifest_with_invalid_lsp_server() -> PluginManifest {
    let toml = r#"
[plugin]
id = "community.broken"
name = "Broken"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.protocol_hosts.lsp]]
id = "broken"
command = "   "
"#;
    parse_manifest(toml, "broken.manifest.toml").unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn wire_lsp_contributions_registers_every_contribution() {
    let forge = MinimalForge::new();
    let manifest = manifest_with_two_lsp_servers();
    let outcomes = wire_lsp_contributions(&forge.runtime.context, &[&manifest]).await;
    assert_eq!(outcomes.len(), 2);
    for o in &outcomes {
        assert_eq!(o.status, LspWireStatus::Ok, "outcome was {o:?}");
        assert_eq!(o.plugin_id, "community.rust");
    }
    let reply = forge
        .ipc_call("com.nexus.lsp", "list_servers", serde_json::json!({}))
        .await
        .unwrap();
    let names: Vec<String> = reply
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"rust-analyzer".to_string()));
    assert!(names.contains(&"alt-server".to_string()));
}

#[tokio::test(flavor = "current_thread")]
async fn wire_lsp_contributions_reports_invalid_command_as_skip_status() {
    let forge = MinimalForge::new();
    let manifest = manifest_with_invalid_lsp_server();
    let outcomes = wire_lsp_contributions(&forge.runtime.context, &[&manifest]).await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].status, LspWireStatus::InvalidCommand);
    let reply = forge
        .ipc_call("com.nexus.lsp", "list_servers", serde_json::json!({}))
        .await
        .unwrap();
    assert!(reply.as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn wire_then_unwire_is_idempotent_round_trip() {
    let forge = MinimalForge::new();
    let manifest = manifest_with_two_lsp_servers();

    let wired = wire_lsp_contributions(&forge.runtime.context, &[&manifest]).await;
    assert!(wired.iter().all(|o| o.status == LspWireStatus::Ok));

    let unwired = unwire_lsp_contributions_for_plugin(&forge.runtime.context, &manifest).await;
    assert_eq!(unwired.len(), 2);
    for o in &unwired {
        assert_eq!(o.status, LspWireStatus::Ok, "outcome was {o:?}");
    }

    let reply = forge
        .ipc_call("com.nexus.lsp", "list_servers", serde_json::json!({}))
        .await
        .unwrap();
    assert!(reply.as_array().unwrap().is_empty());

    let second = unwire_lsp_contributions_for_plugin(&forge.runtime.context, &manifest).await;
    assert_eq!(second.len(), 2);
    for o in &second {
        assert_eq!(o.status, LspWireStatus::NotFound);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn unwire_refuses_when_called_by_different_plugin() {
    let forge = MinimalForge::new();
    let manifest_a = manifest_with_two_lsp_servers();
    wire_lsp_contributions(&forge.runtime.context, &[&manifest_a]).await;

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

[[registrations.protocol_hosts.lsp]]
id = "rust-analyzer"
command = "doesnt-matter"

[[registrations.protocol_hosts.lsp]]
id = "alt-server"
command = "doesnt-matter"
"#,
        "intruder.manifest.toml",
    )
    .unwrap();
    let outcomes = unwire_lsp_contributions_for_plugin(&forge.runtime.context, &manifest_b).await;
    assert_eq!(outcomes.len(), 2);
    for o in &outcomes {
        match &o.status {
            LspWireStatus::NotOwnedByPlugin { actual_owner } => {
                assert_eq!(actual_owner, "community.rust");
            }
            other => panic!("expected NotOwnedByPlugin, got {other:?}"),
        }
    }

    let reply = forge
        .ipc_call("com.nexus.lsp", "list_servers", serde_json::json!({}))
        .await
        .unwrap();
    let names: Vec<String> = reply
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"rust-analyzer".to_string()));
    assert!(names.contains(&"alt-server".to_string()));
}

#[tokio::test(flavor = "current_thread")]
async fn wire_handles_empty_manifest_set_without_dispatching() {
    let forge = MinimalForge::new();
    let outcomes = wire_lsp_contributions(&forge.runtime.context, &[]).await;
    assert!(outcomes.is_empty());
}
