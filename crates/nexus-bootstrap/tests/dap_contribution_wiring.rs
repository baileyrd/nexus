//! BL-113 Phase 1c — integration tests for the bootstrap-side
//! `wire_dap_contributions` / `unwire_dap_contributions_for_plugin`
//! pair against a real booted runtime.
//!
//! The DAP host plugin is registered eagerly at bootstrap (see
//! `register_core_plugins` in `crates/nexus-bootstrap/src/lib.rs`),
//! so `MinimalForge::new()` is enough — no extra setup beyond synthesising
//! community-plugin manifests that declare DAP contributions.

#![cfg(not(target_arch = "wasm32"))]

#[path = "common/mod.rs"]
mod common;

use common::MinimalForge;
use nexus_bootstrap::dap_contribution_wiring::{
    unwire_dap_contributions_for_plugin, wire_dap_contributions, DapWireStatus,
};
use nexus_plugins::{parse_manifest, PluginManifest};

fn manifest_with_two_dap_adapters() -> PluginManifest {
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
command = "codelldb"
args = ["--port", "0"]
file_types = ["rs"]

[[registrations.protocol_hosts.dap]]
id = "rust-attach"
command = "codelldb"
args = ["--attach"]
file_types = ["rs"]
"#;
    parse_manifest(toml, "rust-debug.manifest.toml").unwrap()
}

fn manifest_with_invalid_dap_adapter() -> PluginManifest {
    // Manifest-level parsing accepts the entry; the host's
    // `register_contributed` is what rejects empty command.
    let toml = r#"
[plugin]
id = "community.broken"
name = "Broken"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.protocol_hosts.dap]]
id = "broken"
command = "   "
"#;
    parse_manifest(toml, "broken.manifest.toml").unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn wire_dap_contributions_registers_every_contribution() {
    let forge = MinimalForge::new();
    let manifest = manifest_with_two_dap_adapters();
    let outcomes = wire_dap_contributions(&forge.runtime.context, &[&manifest]).await;
    assert_eq!(outcomes.len(), 2);
    for o in &outcomes {
        assert_eq!(o.status, DapWireStatus::Ok, "outcome was {o:?}");
        assert_eq!(o.plugin_id, "community.rust-debug");
    }
    // The host exposes the merged set via list_adapters — verify both
    // contributions landed.
    let reply = forge
        .ipc_call("com.nexus.dap", "list_adapters", serde_json::json!({}))
        .await
        .unwrap();
    let names: Vec<String> = reply
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"rust".to_string()));
    assert!(names.contains(&"rust-attach".to_string()));
}

#[tokio::test(flavor = "current_thread")]
async fn wire_dap_contributions_reports_invalid_command_as_skip_status() {
    let forge = MinimalForge::new();
    let manifest = manifest_with_invalid_dap_adapter();
    let outcomes = wire_dap_contributions(&forge.runtime.context, &[&manifest]).await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].status, DapWireStatus::InvalidCommand);
    // The bad entry never landed in the host's adapter map.
    let reply = forge
        .ipc_call("com.nexus.dap", "list_adapters", serde_json::json!({}))
        .await
        .unwrap();
    assert!(reply.as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn wire_then_unwire_is_idempotent_round_trip() {
    let forge = MinimalForge::new();
    let manifest = manifest_with_two_dap_adapters();

    // Wire — both adapters register.
    let wired = wire_dap_contributions(&forge.runtime.context, &[&manifest]).await;
    assert!(wired.iter().all(|o| o.status == DapWireStatus::Ok));

    // Unwire — both adapters successfully unregister.
    let unwired = unwire_dap_contributions_for_plugin(&forge.runtime.context, &manifest).await;
    assert_eq!(unwired.len(), 2);
    for o in &unwired {
        assert_eq!(o.status, DapWireStatus::Ok, "outcome was {o:?}");
    }

    // The host's adapter map is empty after the unwire pass.
    let reply = forge
        .ipc_call("com.nexus.dap", "list_adapters", serde_json::json!({}))
        .await
        .unwrap();
    assert!(reply.as_array().unwrap().is_empty());

    // A second unwire is a no-op pattern: every adapter is now
    // not_found because we just removed them. The pass shouldn't
    // panic / dispatch-error.
    let second = unwire_dap_contributions_for_plugin(&forge.runtime.context, &manifest).await;
    assert_eq!(second.len(), 2);
    for o in &second {
        assert_eq!(o.status, DapWireStatus::NotFound);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn unwire_refuses_when_called_by_different_plugin() {
    let forge = MinimalForge::new();
    let manifest_a = manifest_with_two_dap_adapters();

    // Plugin A registers two adapters.
    wire_dap_contributions(&forge.runtime.context, &[&manifest_a]).await;

    // Plugin B tries to unwire them — same adapter names, different
    // plugin id. Host refuses with `not_owned_by_plugin` and reports
    // the real owner.
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

[[registrations.protocol_hosts.dap]]
id = "rust"
command = "doesnt-matter"

[[registrations.protocol_hosts.dap]]
id = "rust-attach"
command = "doesnt-matter"
"#,
        "intruder.manifest.toml",
    )
    .unwrap();
    let outcomes = unwire_dap_contributions_for_plugin(&forge.runtime.context, &manifest_b).await;
    assert_eq!(outcomes.len(), 2);
    for o in &outcomes {
        match &o.status {
            DapWireStatus::NotOwnedByPlugin { actual_owner } => {
                assert_eq!(actual_owner, "community.rust-debug");
            }
            other => panic!("expected NotOwnedByPlugin, got {other:?}"),
        }
    }

    // A's adapters survive the failed intruder attempt.
    let reply = forge
        .ipc_call("com.nexus.dap", "list_adapters", serde_json::json!({}))
        .await
        .unwrap();
    let names: Vec<String> = reply
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"rust".to_string()));
    assert!(names.contains(&"rust-attach".to_string()));
}

#[tokio::test(flavor = "current_thread")]
async fn wire_handles_empty_manifest_set_without_dispatching() {
    let forge = MinimalForge::new();
    let outcomes = wire_dap_contributions(&forge.runtime.context, &[]).await;
    assert!(outcomes.is_empty());
}

/// BL-113 — the in-tree reference plugin at
/// `plugins/first-party-dap-python/manifest.toml` parses, wires
/// through `register_adapter`, and surfaces on `list_adapters` with
/// the shell-only fields packed into `metadata` so the launch form
/// can read them without an extra IPC.
#[tokio::test(flavor = "current_thread")]
async fn first_party_dap_python_plugin_wires_and_surfaces_metadata() {
    // Resolve `plugins/first-party-dap-python/manifest.toml` relative
    // to the workspace root. `CARGO_MANIFEST_DIR` is
    // `crates/nexus-bootstrap` when this test runs, so two `..` hops
    // get us back to the repo root.
    let manifest_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("plugins")
        .join("first-party-dap-python")
        .join("manifest.toml");
    let toml = std::fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", manifest_path.display()));
    let manifest = parse_manifest(&toml, manifest_path.to_str().unwrap()).unwrap();
    assert_eq!(manifest.id, "first-party.dap.python");
    assert_eq!(manifest.registrations.protocol_hosts.dap.len(), 1);
    let dap = &manifest.registrations.protocol_hosts.dap[0];
    assert_eq!(dap.id, "python");
    assert_eq!(dap.command, "python3");
    assert_eq!(dap.args, ["-m", "debugpy.adapter"]);
    assert_eq!(
        dap.launch_config_schema.as_deref(),
        Some("./launch.schema.json"),
    );

    let forge = MinimalForge::new();
    let outcomes = wire_dap_contributions(&forge.runtime.context, &[&manifest]).await;
    assert_eq!(outcomes.len(), 1);
    assert!(
        matches!(outcomes[0].status, DapWireStatus::Ok),
        "first-party.dap.python should wire cleanly, got {:?}",
        outcomes[0].status,
    );

    let reply = forge
        .ipc_call("com.nexus.dap", "list_adapters", serde_json::json!({}))
        .await
        .unwrap();
    let entries = reply.as_array().unwrap();
    let python = entries
        .iter()
        .find(|e| e["name"] == "python")
        .expect("python adapter should appear in list_adapters");
    assert_eq!(python["command"], "python3");
    let metadata = &python["metadata"];
    assert_eq!(metadata["plugin_id"], "first-party.dap.python");
    assert_eq!(metadata["display_name"], "Python (debugpy)");
    assert_eq!(metadata["launch_config_schema"], "./launch.schema.json");
    // `root_markers` was set in the manifest; the wire surface
    // carries it under metadata so the shell can score "this adapter
    // applies to this workspace" without a second IPC.
    assert_eq!(
        metadata["root_markers"],
        serde_json::json!([
            "pyproject.toml",
            "setup.py",
            "requirements.txt",
            ".git",
        ]),
    );
}
