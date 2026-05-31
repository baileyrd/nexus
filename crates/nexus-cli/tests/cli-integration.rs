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
    let meta = engine
        .write_file("notes/test.md", b"# Hello\n\nWorld")
        .unwrap();
    assert_eq!(meta.path, "notes/test.md");
    let content = engine.read_file("notes/test.md").unwrap();
    assert_eq!(content, b"# Hello\n\nWorld");
}

#[test]
fn storage_search_finds_content() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = nexus_storage::StorageEngine::init(tmp.path()).unwrap();
    engine
        .write_file("notes/rust.md", b"# Rust Programming\n\nRust is great.")
        .unwrap();
    engine.rebuild_search_index().unwrap();
    let results = engine.search("rust", 10).unwrap();
    assert!(!results.is_empty());
}

#[test]
fn storage_delete_removes_file() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = nexus_storage::StorageEngine::init(tmp.path()).unwrap();
    engine
        .write_file("notes/delete-me.md", b"temporary")
        .unwrap();
    engine.delete_file("notes/delete-me.md").unwrap();
    assert!(!engine.file_exists("notes/delete-me.md").unwrap());
}

// ---------------------------------------------------------------------------
// WI-40 — MCP parity CLI subcommand smoke tests
//
// Exercise the kernel IPC helpers behind `nexus content update`,
// `nexus content list`, and `nexus tags list`. The CLI handlers are
// thin wrappers over these helpers — end-to-end binary coverage would
// require spawning a child process, which is not how this file rolls.
// ---------------------------------------------------------------------------

#[test]
fn content_update_overwrites_existing_file() {
    let tmp = tempfile::tempdir().unwrap();
    nexus_bootstrap::init_forge(tmp.path()).unwrap();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let runtime = nexus_bootstrap::build_cli_runtime(tmp.path().to_path_buf()).unwrap();
    let invoker = runtime.invoker();

    // Create then update — mirrors `nexus content create` followed by
    // `nexus content update`.
    rt.block_on(nexus_bootstrap::storage::write_file(
        &*invoker,
        "notes/a.md",
        b"first",
    ))
    .unwrap();
    let meta = rt
        .block_on(nexus_bootstrap::storage::write_file(
            &*invoker,
            "notes/a.md",
            b"second",
        ))
        .unwrap();
    assert_eq!(meta.path, "notes/a.md");

    let bytes = rt
        .block_on(nexus_bootstrap::storage::read_file(&*invoker, "notes/a.md"))
        .unwrap();
    assert_eq!(bytes, b"second");
}

#[test]
fn content_list_with_prefix_filters() {
    let tmp = tempfile::tempdir().unwrap();
    nexus_bootstrap::init_forge(tmp.path()).unwrap();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let runtime = nexus_bootstrap::build_cli_runtime(tmp.path().to_path_buf()).unwrap();
    let invoker = runtime.invoker();

    rt.block_on(nexus_bootstrap::storage::write_file(
        &*invoker,
        "notes/one.md",
        b"1",
    ))
    .unwrap();
    rt.block_on(nexus_bootstrap::storage::write_file(
        &*invoker,
        "notes/two.md",
        b"2",
    ))
    .unwrap();
    rt.block_on(nexus_bootstrap::storage::write_file(
        &*invoker,
        "other/three.md",
        b"3",
    ))
    .unwrap();

    let all = rt
        .block_on(nexus_bootstrap::storage::query_files_with_prefix(
            &*invoker, "",
        ))
        .unwrap();
    assert!(all.len() >= 3);

    let filtered = rt
        .block_on(nexus_bootstrap::storage::query_files_with_prefix(
            &*invoker, "notes/",
        ))
        .unwrap();
    let paths: Vec<&str> = filtered.iter().map(|r| r.path.as_str()).collect();
    assert!(paths.iter().all(|p| p.starts_with("notes/")));
    assert!(paths.contains(&"notes/one.md"));
    assert!(paths.contains(&"notes/two.md"));
    assert!(!paths.contains(&"other/three.md"));
}

#[test]
fn tags_list_returns_tag_occurrences() {
    let tmp = tempfile::tempdir().unwrap();
    nexus_bootstrap::init_forge(tmp.path()).unwrap();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let runtime = nexus_bootstrap::build_cli_runtime(tmp.path().to_path_buf()).unwrap();
    let invoker = runtime.invoker();

    // An inline-tagged note and a frontmatter-tagged note.
    rt.block_on(nexus_bootstrap::storage::write_file(
        &*invoker,
        "notes/inline.md",
        b"# Inline\n\nA #project tagged line.\n",
    ))
    .unwrap();
    rt.block_on(nexus_bootstrap::storage::write_file(
        &*invoker,
        "notes/front.md",
        b"---\ntags: [project]\n---\n# Front\n",
    ))
    .unwrap();

    let hits = rt
        .block_on(nexus_bootstrap::storage::query_tags(&*invoker, "project"))
        .unwrap();
    assert!(
        !hits.is_empty(),
        "expected at least one occurrence of #project"
    );
    assert!(hits.iter().all(|t| t.name == "project"));
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

/// WI-39: scaffold the modern script (sandboxed JS/TS) template and verify
/// the 5-file layout matches what the shell's plugin loader expects (mirror
/// of `shell/src/plugins/community/hello-world/`). We don't run `pnpm install
/// && pnpm build` inline — @nexus/extension-api isn't published to npm yet
/// (Phase 5 WI-44), and the sandbox for test runs shouldn't depend on the
/// network. The live-build check is documented in the WI-39 commit message
/// and was exercised manually against a local tarball.
#[test]
fn plugin_scaffold_script_template_matches_shell_layout() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("com.example.hello");
    let config = nexus_plugins::ScaffoldConfig {
        plugin_id: "com.example.hello".to_string(),
        plugin_name: "Hello".to_string(),
        author: "Tester".to_string(),
        description: "Hello — Nexus plugin.".to_string(),
    };
    nexus_plugins::scaffold(&out, nexus_plugins::PluginTemplate::Script, &config).unwrap();

    // Author-facing files.
    assert!(out.join("plugin.json").is_file(), "plugin.json missing");
    assert!(out.join("index.ts").is_file(), "index.ts missing");
    assert!(out.join("package.json").is_file(), "package.json missing");
    assert!(out.join("tsconfig.json").is_file(), "tsconfig.json missing");
    assert!(out.join("README.md").is_file(), "README.md missing");

    // No WASM-template leakage.
    assert!(!out.join("Cargo.toml").exists());
    assert!(!out.join("manifest.toml").exists());
    assert!(!out.join("src").exists());

    // Sanity check the manifest matches the sandboxed-plugin contract.
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("plugin.json")).unwrap())
            .expect("plugin.json must be valid JSON");
    assert_eq!(manifest["sandboxed"], true);
    assert_eq!(manifest["apiVersion"], 1);
    assert_eq!(manifest["main"], "index.js");
    assert_eq!(manifest["id"], "com.example.hello");
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
