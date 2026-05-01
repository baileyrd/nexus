//! Regression test for issue #72 — path-traversal in storage IPC handlers.
//!
//! Before the fix, `StorageEngine::write_file`, `write_raw`, `read_file`, and
//! `delete_file` all built their absolute target as `forge.root().join(path)`.
//! `Path::join` does not normalise `..`, so a caller-supplied relpath like
//! `"../../etc/passwd"` reached `std::fs::*` and was resolved at the syscall
//! level — escaping forge confinement entirely. A community plugin holding
//! only `ipc.call` could read, overwrite, or delete files anywhere the host
//! user could.
//!
//! These tests drive the IPC handlers (not the engine methods directly) so
//! that the regression covers the whole reachable surface — the path that an
//! attacker actually has via `ctx.ipc_call("com.nexus.storage", ...)`.

use std::sync::Arc;

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, PluginError};
use nexus_storage::{
    StorageConfig, StorageCorePlugin, StorageEngine,
    core_plugin::{
        HANDLER_DELETE_FILE, HANDLER_NOTE_APPEND, HANDLER_READ_FILE,
        HANDLER_WRITE_FILE, HANDLER_WRITE_VAULT_FILE,
    },
};

fn boot_plugin(forge_root: &std::path::Path) -> StorageCorePlugin {
    drop(StorageEngine::init(forge_root).expect("init forge"));
    let bus = Arc::new(EventBus::new(16));
    let mut plugin =
        StorageCorePlugin::new(forge_root.to_path_buf(), &StorageConfig::default(), bus);
    plugin.on_init().expect("on_init");
    plugin
}

/// Paths that a malicious caller might use to escape the forge root. Each
/// shape exercises a different class of `Path::component` rejection in
/// [`nexus_types::paths::resolve_within`].
fn evil_paths() -> Vec<&'static str> {
    vec![
        "../escape.txt",
        "../../escape.txt",
        "notes/../../escape.txt",
        "/etc/passwd",
        "./escape.txt",
    ]
}

fn assert_rejected(err: PluginError, path: &str, op: &str) {
    match err {
        PluginError::ExecutionFailed { reason, .. } => {
            assert!(
                reason.contains("invalid relpath"),
                "{op} on {path:?}: expected resolve_within rejection, got: {reason}"
            );
        }
        other => panic!("{op} on {path:?}: unexpected error variant: {other:?}"),
    }
}

#[test]
fn write_file_rejects_traversal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut plugin = boot_plugin(dir.path());

    for evil in evil_paths() {
        let args = serde_json::json!({ "path": evil, "bytes": [0u8, 1u8, 2u8] });
        let err = plugin
            .dispatch(HANDLER_WRITE_FILE, &args)
            .expect_err(&format!("write_file must reject {evil:?}"));
        assert_rejected(err, evil, "write_file");
    }

    // Positive control: a normal forge-relative path still works.
    let ok = serde_json::json!({ "path": "notes/ok.md", "bytes": [104u8, 105u8] });
    plugin
        .dispatch(HANDLER_WRITE_FILE, &ok)
        .expect("normal write_file must still succeed");
    assert!(dir.path().join("notes/ok.md").exists());
}

#[test]
fn write_vault_file_rejects_traversal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut plugin = boot_plugin(dir.path());

    // Post-#80, `write_vault_file` rejects anything outside `.forge/`
    // *before* the engine's `resolve_within` check fires, so most of
    // the `evil_paths()` set surfaces with the new "outside the
    // .forge/" message (the .forge/ gate; semantically the same
    // outcome — the call doesn't touch disk). To keep this test
    // pinned on the traversal layer specifically, use payloads that
    // start with `.forge/` (passing the prefix gate) but contain
    // `..` to exercise resolve_within.
    let evil_traversals_under_forge = [
        ".forge/../escape.txt",
        ".forge/sub/../../escape.txt",
    ];
    for evil in evil_traversals_under_forge {
        let args = serde_json::json!({ "path": evil, "bytes": [0u8] });
        let err = plugin
            .dispatch(HANDLER_WRITE_VAULT_FILE, &args)
            .expect_err(&format!("write_vault_file must reject {evil:?}"));
        assert_rejected(err, evil, "write_vault_file");
    }
}

#[test]
fn read_file_rejects_traversal() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Plant a file outside the forge root that an attacker might want to
    // exfiltrate. Confined `read_file` must NOT be able to reach it via
    // `..` traversal even though the on-disk file is real.
    let outside = dir.path().parent().unwrap().join("outside-secret.txt");
    std::fs::write(&outside, b"secret").expect("seed outside file");
    let mut plugin = boot_plugin(dir.path());

    let attempts = [
        format!(
            "../{}",
            outside.file_name().unwrap().to_string_lossy()
        ),
        "../../etc/passwd".to_string(),
        "/etc/passwd".to_string(),
    ];
    for evil in attempts {
        let args = serde_json::json!({ "path": evil });
        let err = plugin
            .dispatch(HANDLER_READ_FILE, &args)
            .expect_err(&format!("read_file must reject {evil:?}"));
        assert_rejected(err, &evil, "read_file");
    }

    // Cleanup the planted file regardless of test outcome.
    let _ = std::fs::remove_file(&outside);
}

#[test]
fn delete_file_rejects_traversal() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Plant a sibling file that delete_file must NOT be able to reach.
    let outside = dir.path().parent().unwrap().join("outside-victim.txt");
    std::fs::write(&outside, b"do not delete").expect("seed outside file");
    let mut plugin = boot_plugin(dir.path());

    let evil_path = format!(
        "../{}",
        outside.file_name().unwrap().to_string_lossy()
    );
    let args = serde_json::json!({ "path": evil_path });
    let err = plugin
        .dispatch(HANDLER_DELETE_FILE, &args)
        .expect_err(&format!("delete_file must reject {evil_path:?}"));
    assert_rejected(err, &evil_path, "delete_file");

    assert!(
        outside.exists(),
        "delete_file must not have reached the outside file"
    );
    let _ = std::fs::remove_file(&outside);
}

#[test]
fn note_append_rejects_traversal() {
    // `note_append` reads then writes; both halves now go through
    // `resolve_within`. The first call (read) should surface the
    // rejection before we even attempt to touch a file.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut plugin = boot_plugin(dir.path());

    for evil in evil_paths() {
        let args = serde_json::json!({ "path": evil, "snippet": "x" });
        let err = plugin
            .dispatch(HANDLER_NOTE_APPEND, &args)
            .expect_err(&format!("note_append must reject {evil:?}"));
        assert_rejected(err, evil, "note_append");
    }
}
