//! Regression test for `HANDLER_READ_FILE` on a non-existent path.
//!
//! Before this fix, the storage plugin propagated `StorageError::FileNotFound`
//! as a `PluginError::ExecutionFailed`, which the Tauri IPC dispatcher in
//! `crates/nexus-app/src/plugins.rs` collapses into
//! `IpcError::PluginCrashedDuringCall`. The shell's workspace-persistence
//! layer then logged a scary "plugin crashed" warning during a normal boot
//! (the `.forge/workspace.json` file doesn't exist on first run).
//!
//! The handler now returns `{ "bytes": null }` for missing files so callers
//! can distinguish "missing" from "crashed" without scraping error strings.

use std::sync::Arc;

use nexus_kernel::EventBus;
use nexus_plugins::CorePlugin;
use nexus_storage::{
    StorageConfig, StorageCorePlugin, StorageEngine,
    core_plugin::HANDLER_READ_FILE,
};

#[test]
fn read_file_on_missing_path_returns_bytes_null() {
    let dir = tempfile::tempdir().expect("tempdir");
    // StorageCorePlugin::on_init opens its own engine handle and therefore
    // its own lockfile; drop the initialising engine before handing over.
    drop(StorageEngine::init(dir.path()).expect("init forge"));

    let bus = Arc::new(EventBus::new(16));
    let mut plugin =
        StorageCorePlugin::new(dir.path().to_path_buf(), &StorageConfig::default(), bus);
    plugin.on_init().expect("on_init");

    let args = serde_json::json!({ "path": "notes/definitely-does-not-exist.md" });
    let resp = plugin
        .dispatch(HANDLER_READ_FILE, &args)
        .expect("read_file on missing path must not error");

    assert_eq!(
        resp,
        serde_json::json!({ "bytes": null }),
        "missing file must surface as typed null, not as an error",
    );
}

#[test]
fn read_file_on_existing_path_returns_bytes_array() {
    let dir = tempfile::tempdir().expect("tempdir");
    {
        let engine = StorageEngine::init(dir.path()).expect("init forge");
        engine
            .write_file("notes/hello.md", b"hello world")
            .expect("seed file");
    }

    let bus = Arc::new(EventBus::new(16));
    let mut plugin =
        StorageCorePlugin::new(dir.path().to_path_buf(), &StorageConfig::default(), bus);
    plugin.on_init().expect("on_init");

    let args = serde_json::json!({ "path": "notes/hello.md" });
    let resp = plugin
        .dispatch(HANDLER_READ_FILE, &args)
        .expect("read_file on existing path");

    let bytes = resp
        .get("bytes")
        .and_then(|v| v.as_array())
        .expect("bytes must be a JSON array for existing files");
    let decoded: Vec<u8> = bytes
        .iter()
        .map(|v| u8::try_from(v.as_u64().unwrap()).unwrap())
        .collect();
    assert_eq!(decoded, b"hello world");
}
