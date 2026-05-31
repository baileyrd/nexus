//! PRD-04 smoke test: verifies the public API surface and plugin lifecycle
//! for the nexus-plugins crate.

use std::path::Path;

use nexus_plugins::{
    HotReloader, PluginData, PluginError, PluginLoader, PluginManager, PluginManagerConfig,
    ReloadEvent, SettingsManager, WasmConfig, WasmSandbox,
};

// ─── Shared helper ────────────────────────────────────────────────────────────

fn setup_smoke_plugin(plugin_id: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let plugin_dir = tmp.path().join(plugin_id);
    std::fs::create_dir_all(&plugin_dir).unwrap();

    let wasm_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/minimal-plugin.wasm");
    std::fs::copy(&wasm_src, plugin_dir.join("test.wasm")).unwrap();

    let manifest = format!(
        r#"
[plugin]
id = "{plugin_id}"
name = "Smoke Test Plugin"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[capabilities]
required = ["kv.read", "kv.write"]

[wasm]
module = "test.wasm"

[[registrations.cli_subcommand]]
id = "{plugin_id}.echo"
handler_id = 100
description = "Echo command"

[lifecycle]
on_init = true
on_start = true
on_stop = true
"#
    );
    std::fs::write(plugin_dir.join("manifest.toml"), manifest).unwrap();
    (tmp, plugin_dir)
}

fn no_reload_config() -> PluginManagerConfig {
    PluginManagerConfig {
        hot_reload: false,
        ..Default::default()
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn public_type_surface_is_accessible() {
    // Verify all major public types from the crate are importable and
    // constructible without errors.
    let _: PluginManagerConfig = PluginManagerConfig::default();
    let tmp = tempfile::tempdir().unwrap();
    let _loader = PluginLoader::new(tmp.path());
    let _settings = SettingsManager::new();
    let _mgr = PluginManager::new(tmp.path(), &no_reload_config()).unwrap();

    // ReloadEvent is a plain struct — construct one to verify it's accessible.
    let _event = ReloadEvent {
        plugin_id: "test".to_string(),
        wasm_path: tmp.path().join("test.wasm"),
    };

    // HotReloader::start on a non-existent path should succeed.
    let _reloader = HotReloader::start(Path::new("/nonexistent/smoke"), 50).unwrap();
}

#[test]
fn manifest_parse_and_validate_roundtrip() {
    let (tmp, plugin_dir) = setup_smoke_plugin("com.smoke.manifest");

    // load_manifest + validate should both succeed on a well-formed manifest.
    let manifest = nexus_plugins::load_manifest(&plugin_dir.join("manifest.toml")).unwrap();
    nexus_plugins::validate(&manifest, &plugin_dir).unwrap();

    assert_eq!(manifest.id, "com.smoke.manifest");
    assert_eq!(manifest.name, "Smoke Test Plugin");
    assert_eq!(manifest.version, "1.0.0");
    assert_eq!(manifest.api_version, "1");
    assert_eq!(manifest.registrations.cli_subcommands.len(), 1);
    assert_eq!(
        manifest.registrations.cli_subcommands[0].id,
        "com.smoke.manifest.echo"
    );
    assert!(manifest.lifecycle.on_init);
    assert!(manifest.lifecycle.on_start);
    assert!(manifest.lifecycle.on_stop);

    // _tmp must stay alive until the end of the test
    drop(tmp);
}

#[test]
fn wasm_sandbox_rejects_invalid_bytes() {
    let config = WasmConfig {
        module: "test.wasm".to_string(),
        memory_mb: 16,
        fuel: 10_000_000,
        max_execution_ms: 5_000,
    };
    let pd = PluginData {
        plugin_id: "com.smoke.invalid".to_string(),
        ..Default::default()
    };
    let result = WasmSandbox::new(b"invalid", &config, pd);
    assert!(
        matches!(result, Err(PluginError::WasmLoadFailed { .. })),
        "expected WasmLoadFailed, got: {result:?}"
    );
}

#[test]
fn settings_schema_validation_works() {
    let schema_json = r#"{
        "type": "object",
        "properties": {
            "threshold": { "type": "integer", "minimum": 0 }
        },
        "required": ["threshold"]
    }"#;

    let mut mgr = SettingsManager::new();
    mgr.register_schema("com.smoke.settings", schema_json)
        .unwrap();

    // Valid settings should pass.
    mgr.validate("com.smoke.settings", &serde_json::json!({"threshold": 5}))
        .unwrap();

    // Missing required field should fail.
    let err = mgr
        .validate("com.smoke.settings", &serde_json::json!({}))
        .unwrap_err();
    assert!(
        matches!(err, PluginError::SettingsInvalid { .. }),
        "expected SettingsInvalid, got: {err:?}"
    );

    // Wrong type should fail.
    let err = mgr
        .validate(
            "com.smoke.settings",
            &serde_json::json!({"threshold": "not-a-number"}),
        )
        .unwrap_err();
    assert!(
        matches!(err, PluginError::SettingsInvalid { .. }),
        "expected SettingsInvalid, got: {err:?}"
    );
}

#[test]
fn full_plugin_lifecycle() {
    let (tmp, plugin_dir) = setup_smoke_plugin("com.smoke.lifecycle");
    let mut mgr = PluginManager::new(tmp.path(), &no_reload_config()).unwrap();

    // Load
    let info = mgr.load(&plugin_dir).unwrap();
    assert_eq!(info.id, "com.smoke.lifecycle");

    // Dispatch CLI (echo handler returns args unchanged)
    let args = serde_json::json!({"smoke": "test"});
    let result = mgr.dispatch_cli("com.smoke.lifecycle.echo", &args).unwrap();
    assert_eq!(result, args, "echo handler should return args unchanged");

    // List
    let list = mgr.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "com.smoke.lifecycle");

    // Get
    let got = mgr.get("com.smoke.lifecycle").unwrap();
    assert_eq!(got.id, "com.smoke.lifecycle");

    // Shutdown
    mgr.shutdown().unwrap();
    assert!(mgr.list().is_empty());
}

#[test]
fn load_all_scans_directory() {
    let (tmp, _plugin_dir) = setup_smoke_plugin("com.smoke.scanme");
    let mut mgr = PluginManager::new(tmp.path(), &no_reload_config()).unwrap();

    let infos = mgr.load_all().unwrap();
    assert_eq!(
        infos.len(),
        1,
        "expected exactly one plugin, got: {infos:?}"
    );
    assert_eq!(infos[0].id, "com.smoke.scanme");
}

#[test]
fn plugin_error_variants_display_correctly() {
    let errors: Vec<PluginError> = vec![
        PluginError::PluginNotFound("missing".to_string()),
        PluginError::DuplicatePlugin("dupe".to_string()),
        PluginError::WasmLoadFailed {
            plugin_id: "bad".to_string(),
            reason: "invalid bytes".to_string(),
        },
        PluginError::SettingsInvalid {
            plugin_id: "cfg".to_string(),
            reason: "missing field".to_string(),
        },
        PluginError::ReloadFailed {
            plugin_id: "hot".to_string(),
            reason: "compile error".to_string(),
        },
        PluginError::ManifestNotFound("no/such/file.toml".to_string()),
    ];

    for err in &errors {
        let msg = err.to_string();
        assert!(!msg.is_empty(), "Display for {err:?} should be non-empty");
    }
}

#[test]
fn scaffold_types_accessible() {
    let _: Option<nexus_plugins::PluginTemplate> = None;
    let _: Option<nexus_plugins::ScaffoldConfig> = None;
}

#[test]
fn scaffold_generates_compilable_project() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("smoke-plugin");
    let config = nexus_plugins::ScaffoldConfig {
        plugin_id: "com.test.scaffold.smoke".to_string(),
        plugin_name: "Scaffold Smoke".to_string(),
        author: "Tester".to_string(),
        description: "Smoke test plugin.".to_string(),
    };
    nexus_plugins::scaffold(&out, nexus_plugins::PluginTemplate::Community, &config).unwrap();
    let manifest = std::fs::read_to_string(out.join("manifest.toml")).unwrap();
    assert!(manifest.contains("com.test.scaffold.smoke"));
    let lib_rs = std::fs::read_to_string(out.join("src/lib.rs")).unwrap();
    assert!(lib_rs.contains("nexus_dispatch"));
    assert!(lib_rs.contains("nexus_alloc"));
}
