//! CLI integration tests

#[test]
fn forge_init_creates_forge_structure() {
    let tmp = tempfile::tempdir().unwrap();
    let forge_root = tmp.path().join("test-forge");
    nexus_storage::StorageEngine::init(&forge_root).unwrap();
    assert!(forge_root.join(".forge").is_dir());
    assert!(forge_root.join("notes").is_dir());
    assert!(forge_root.join(".forge/index.db").is_file());
}

#[test]
fn storage_write_read_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = nexus_storage::StorageEngine::init(tmp.path()).unwrap();
    let meta = engine.write_file("notes/test.md", b"# Hello\n\nWorld").unwrap();
    assert_eq!(meta.path, "notes/test.md");
    let content = engine.read_file("notes/test.md").unwrap();
    assert_eq!(content, b"# Hello\n\nWorld");
}

#[test]
fn storage_search_finds_content() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = nexus_storage::StorageEngine::init(tmp.path()).unwrap();
    engine.write_file("notes/rust.md", b"# Rust Programming\n\nRust is great.").unwrap();
    engine.rebuild_search_index().unwrap();
    let results = engine.search("rust", 10).unwrap();
    assert!(!results.is_empty());
}

#[test]
fn storage_delete_removes_file() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = nexus_storage::StorageEngine::init(tmp.path()).unwrap();
    engine.write_file("notes/delete-me.md", b"temporary").unwrap();
    engine.delete_file("notes/delete-me.md").unwrap();
    assert!(!engine.file_exists("notes/delete-me.md").unwrap());
}

#[test]
fn plugin_scaffold_generates_project() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("my-plugin");
    let config = nexus_plugins::ScaffoldConfig {
        plugin_id: "com.test.cli".to_string(),
        plugin_name: "CLI Test".to_string(),
        author: "Tester".to_string(),
        description: "Test plugin".to_string(),
    };
    nexus_plugins::scaffold(&out, nexus_plugins::PluginTemplate::Community, &config).unwrap();
    assert!(out.join("Cargo.toml").is_file());
    assert!(out.join("manifest.toml").is_file());
    assert!(out.join("src/lib.rs").is_file());
}

#[test]
fn plugin_load_and_dispatch() {
    // Copy WASM fixture from nexus-plugins test fixtures
    let wasm_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../nexus-plugins/tests/fixtures/minimal-plugin.wasm");
    if !wasm_src.exists() {
        // Skip if fixture not built
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let plugin_dir = tmp.path().join("com.test.cli");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::copy(&wasm_src, plugin_dir.join("test.wasm")).unwrap();

    let manifest = r#"
[plugin]
id = "com.test.cli"
name = "CLI Test"
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
"#;
    std::fs::write(plugin_dir.join("manifest.toml"), manifest).unwrap();

    let config = nexus_plugins::PluginManagerConfig {
        hot_reload: false,
        ..Default::default()
    };
    let mut mgr = nexus_plugins::PluginManager::new(tmp.path(), &config).unwrap();
    let info = mgr.load(&plugin_dir).unwrap();
    assert_eq!(info.id, "com.test.cli");

    let args = serde_json::json!({"test": true});
    let result = mgr.dispatch_ipc("com.test.cli", "echo", &args).unwrap();
    assert_eq!(result, args);
}
