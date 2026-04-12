//! PRD 05 smoke test: walking skeleton exercising all M1 subsystems.

#[test]
fn walking_skeleton_forge_content_lifecycle() {
    let tmp = tempfile::tempdir().unwrap();
    let forge = tmp.path().join("smoke-forge");

    // Init
    let engine = nexus_storage::StorageEngine::init(&forge).unwrap();
    assert!(forge.join(".forge/index.db").exists());

    // Create
    let meta = engine.write_file("notes/welcome.md", b"# Welcome\n\nHello from Nexus.").unwrap();
    assert_eq!(meta.path, "notes/welcome.md");

    // Exists
    assert!(engine.file_exists("notes/welcome.md").unwrap());

    // Read
    let content = engine.read_file("notes/welcome.md").unwrap();
    assert!(String::from_utf8_lossy(&content).contains("Welcome"));

    // Search
    engine.rebuild_search_index().unwrap();
    let results = engine.search("welcome", 10).unwrap();
    assert!(!results.is_empty());

    // Index queries
    let files = engine.query_files(&nexus_storage::FileFilter::default()).unwrap();
    assert_eq!(files.len(), 1);
    let blocks = engine.query_blocks(files[0].id).unwrap();
    assert!(!blocks.is_empty());

    // Delete
    engine.delete_file("notes/welcome.md").unwrap();
    assert!(!engine.file_exists("notes/welcome.md").unwrap());
}

#[test]
fn walking_skeleton_plugin_lifecycle() {
    let wasm_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../nexus-plugins/tests/fixtures/minimal-plugin.wasm");
    if !wasm_src.exists() { return; }

    let tmp = tempfile::tempdir().unwrap();
    let plugin_dir = tmp.path().join("com.test.smoke");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::copy(&wasm_src, plugin_dir.join("test.wasm")).unwrap();

    std::fs::write(plugin_dir.join("manifest.toml"), r#"
[plugin]
id = "com.test.smoke"
name = "Smoke"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[capabilities]
required = ["kv.read", "kv.write"]

[wasm]
module = "test.wasm"

[[registrations.ipc_command]]
id = "echo"
handler_id = 100

[lifecycle]
on_init = true
on_start = true
on_stop = true
"#).unwrap();

    let config = nexus_plugins::PluginManagerConfig { hot_reload: false, ..Default::default() };
    let mut mgr = nexus_plugins::PluginManager::new(tmp.path(), &config).unwrap();

    let info = mgr.load(&plugin_dir).unwrap();
    assert_eq!(info.id, "com.test.smoke");

    let args = serde_json::json!({"key": "value"});
    let result = mgr.dispatch_ipc("com.test.smoke", "echo", &args).unwrap();
    assert_eq!(result, args);

    assert_eq!(mgr.list().len(), 1);
    mgr.shutdown().unwrap();
    assert!(mgr.list().is_empty());
}

#[test]
fn scaffold_produces_valid_project() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("scaffolded");
    let config = nexus_plugins::ScaffoldConfig {
        plugin_id: "com.test.scaffold.cli".to_string(),
        plugin_name: "CLI Scaffold Test".to_string(),
        author: "Smoke".to_string(),
        description: "Smoke test".to_string(),
    };
    nexus_plugins::scaffold(&out, nexus_plugins::PluginTemplate::Community, &config).unwrap();

    let manifest = std::fs::read_to_string(out.join("manifest.toml")).unwrap();
    assert!(manifest.contains("com.test.scaffold.cli"));
    assert!(manifest.contains("community"));

    let lib_rs = std::fs::read_to_string(out.join("src/lib.rs")).unwrap();
    assert!(lib_rs.contains("nexus_dispatch"));
}
