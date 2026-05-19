//! End-to-end tests for the formats core plugin (`com.nexus.formats`)
//! driven through the kernel IPC surface.

use std::io::{Cursor, Write};
use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_formats::PLUGIN_ID;
use nexus_kernel::Ipc as _;
use zip::write::SimpleFileOptions;

const CALL_TIMEOUT: Duration = Duration::from_secs(10);

fn scratch_forge() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init scratch forge");
    dir
}

fn make_zip(path: &std::path::Path, files: &[(&str, &str)]) {
    let f = std::fs::File::create(path).unwrap();
    let mut zw = zip::ZipWriter::new(f);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in files {
        zw.start_file(*name, opts).unwrap();
        zw.write_all(body.as_bytes()).unwrap();
    }
    zw.finish().unwrap();
}

async fn call(
    runtime: &nexus_bootstrap::Runtime,
    command: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, nexus_kernel::IpcError> {
    runtime
        .context
        .ipc_call(PLUGIN_ID, command, args, CALL_TIMEOUT)
        .await
}

#[tokio::test]
async fn import_notion_zip_via_ipc() {
    let dir = scratch_forge();
    let runtime = build_cli_runtime(dir.path().to_path_buf()).expect("runtime");

    // Convince Cursor<Vec<u8>> via a real on-disk zip.
    let zip_path = dir.path().join("export.zip");
    make_zip(
        &zip_path,
        &[(
            "Export/Page abcd1234abcd1234abcd1234abcd1234.md",
            "# Page\n\nBody.\n",
        )],
    );

    let result = call(
        &runtime,
        "import_notion",
        serde_json::json!({ "source": zip_path, "dest": "from-notion" }),
    )
    .await
    .expect("import_notion ok");

    assert_eq!(result["pages_written"].as_u64(), Some(1));
    assert!(dir.path().join("from-notion/Page.md").exists());
    let _ = Cursor::new(Vec::<u8>::new()); // silence unused-import warn under cfg
}

#[tokio::test]
async fn export_notion_via_ipc_round_trips_a_page() {
    let dir = scratch_forge();
    let runtime = build_cli_runtime(dir.path().to_path_buf()).expect("runtime");
    let dest = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("Hello.md"),
        "---\nnotion_id: aaaa1111aaaa1111aaaa1111aaaa1111\n---\n\nBody.\n",
    )
    .unwrap();

    let result = call(
        &runtime,
        "export_notion",
        serde_json::json!({ "dest": dest.path() }),
    )
    .await
    .expect("export_notion ok");

    assert_eq!(result["pages_written"].as_u64(), Some(1));
    assert!(dest
        .path()
        .join("Hello aaaa1111aaaa1111aaaa1111aaaa1111.md")
        .exists());
}

#[tokio::test]
async fn import_with_missing_zip_errors() {
    let dir = scratch_forge();
    let runtime = build_cli_runtime(dir.path().to_path_buf()).expect("runtime");

    let err = call(
        &runtime,
        "import_notion",
        serde_json::json!({ "source": dir.path().join("nope.zip") }),
    )
    .await
    .expect_err("expected error for missing zip");
    let msg = format!("{err}");
    assert!(msg.contains("not found"), "{msg}");
}
