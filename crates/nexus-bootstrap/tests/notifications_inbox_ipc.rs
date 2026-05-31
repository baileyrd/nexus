//! BL-136 / ADR 0029 — end-to-end IPC tests for the
//! `com.nexus.notifications` inbox surface.
//!
//! Pins the wire-shape contract for `inbox_list`, `inbox_mark_read`,
//! `inbox_dismiss`, and `inbox_stats`, and verifies that an explicit-
//! channel `send` writes a row at dispatch time.

use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_kernel::Ipc as _;

const CALL_TIMEOUT: Duration = Duration::from_secs(10);
const PLUGIN_ID: &str = "com.nexus.notifications";

fn scratch_forge() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init scratch forge");
    dir
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
async fn inbox_list_returns_empty_on_fresh_forge() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");
    let resp = call(&runtime, "inbox_list", serde_json::json!({}))
        .await
        .expect("inbox_list");
    assert!(resp.is_array(), "list returns a JSON array");
    assert_eq!(resp.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn inbox_stats_reports_zero_on_fresh_forge() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");
    let resp = call(&runtime, "inbox_stats", serde_json::json!({}))
        .await
        .expect("inbox_stats");
    assert_eq!(resp.get("total").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(resp.get("unread").and_then(|v| v.as_u64()), Some(0));
    assert!(resp
        .get("by_source")
        .and_then(|v| v.as_object())
        .map(|m| m.is_empty())
        .unwrap_or(false));
}

#[tokio::test]
async fn send_then_list_round_trips_through_inbox() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    // Explicit-channel send — desktop transport just publishes a bus
    // event so this doesn't need any network config.
    call(
        &runtime,
        "send",
        serde_json::json!({
            "channel": "desktop",
            "title": "hello",
            "message": "world",
        }),
    )
    .await
    .expect("send");

    let list = call(&runtime, "inbox_list", serde_json::json!({}))
        .await
        .expect("inbox_list");
    let arr = list.as_array().expect("array");
    assert_eq!(arr.len(), 1, "one row landed after send");
    let row = &arr[0];
    assert_eq!(row.get("source").and_then(|v| v.as_str()), Some("override"));
    assert_eq!(row.get("body").and_then(|v| v.as_str()), Some("world"));
    assert_eq!(row.get("title").and_then(|v| v.as_str()), Some("hello"));
    let channels = row.get("channels").and_then(|v| v.as_array()).unwrap();
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].as_str(), Some("desktop"));

    let stats = call(&runtime, "inbox_stats", serde_json::json!({}))
        .await
        .expect("inbox_stats");
    assert_eq!(stats.get("unread").and_then(|v| v.as_u64()), Some(1));
}

#[tokio::test]
async fn mark_read_flips_unread_then_count() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");
    call(
        &runtime,
        "send",
        serde_json::json!({
            "channel": "desktop",
            "message": "x",
        }),
    )
    .await
    .expect("send");
    let list = call(&runtime, "inbox_list", serde_json::json!({}))
        .await
        .expect("list");
    let id = list[0]
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    let resp = call(
        &runtime,
        "inbox_mark_read",
        serde_json::json!({ "ids": [id.clone()] }),
    )
    .await
    .expect("mark_read");
    assert_eq!(resp.get("updated").and_then(|v| v.as_u64()), Some(1));

    let stats = call(&runtime, "inbox_stats", serde_json::json!({}))
        .await
        .expect("stats");
    assert_eq!(stats.get("unread").and_then(|v| v.as_u64()), Some(0));
}

#[tokio::test]
async fn dismiss_also_marks_read() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");
    call(
        &runtime,
        "send",
        serde_json::json!({
            "channel": "desktop",
            "message": "x",
        }),
    )
    .await
    .expect("send");
    let list = call(&runtime, "inbox_list", serde_json::json!({}))
        .await
        .expect("list");
    let id = list[0]
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    let resp = call(
        &runtime,
        "inbox_dismiss",
        serde_json::json!({ "ids": [id] }),
    )
    .await
    .expect("dismiss");
    assert_eq!(resp.get("updated").and_then(|v| v.as_u64()), Some(1));

    // Default `all` filter excludes dismissed by default? No — `all`
    // means all; the dismissed row stays visible until the caller
    // asks for `unread`. Stats `total` skips dismissed rows.
    let stats = call(&runtime, "inbox_stats", serde_json::json!({}))
        .await
        .expect("stats");
    assert_eq!(stats.get("total").and_then(|v| v.as_u64()), Some(0));
}

#[tokio::test]
async fn inbox_list_rejects_unknown_status_filter() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");
    let err = call(
        &runtime,
        "inbox_list",
        serde_json::json!({ "status": "bogus" }),
    )
    .await
    .unwrap_err();
    let msg = format!("{err:?}");
    assert!(msg.contains("unknown status"), "got: {msg}");
}
