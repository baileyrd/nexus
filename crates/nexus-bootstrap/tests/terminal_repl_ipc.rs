//! BL-142 Phase 1 — end-to-end tests for the REPL surface on
//! `com.nexus.terminal`.
//!
//! Validates the lifecycle wrappers (`repl_start` / `repl_eval` /
//! `repl_stop` / `repl_list`) and the read-only guards that prevent
//! REPL handlers from being pointed at a regular terminal session.
//! The "eval streams real output" test spawns Python 3 if available;
//! it skips with a printed note otherwise.

use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_kernel::{Ipc as _, IpcError};
use serde_json::{json, Value};

const CALL_TIMEOUT: Duration = Duration::from_secs(5);
const TERMINAL_PLUGIN_ID: &str = "com.nexus.terminal";

fn scratch_forge() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init forge");
    dir
}

async fn call(
    runtime: &nexus_bootstrap::Runtime,
    command: &str,
    args: Value,
) -> Result<Value, IpcError> {
    runtime
        .context
        .ipc_call(TERMINAL_PLUGIN_ID, command, args, CALL_TIMEOUT)
        .await
}

fn has_program(name: &str) -> bool {
    std::process::Command::new(name)
        .arg("--version")
        .output()
        .is_ok()
}

/// `cat` is on every POSIX system and acts as the perfect deterministic
/// "kernel" for lifecycle tests — it echoes stdin back to stdout
/// without any per-language prompt logic. We don't need a true REPL for
/// the start/stop/list tests; we just need a long-running process the
/// PTY can attach to.
#[cfg(unix)]
const TEST_KERNEL: (&str, &[&str]) = ("cat", &[]);

/// 1) Lifecycle — `repl_start` returns an id + lang echo; `repl_list`
///    surfaces the entry; `repl_stop` removes it.
#[cfg(unix)]
#[tokio::test]
async fn repl_lifecycle_start_list_stop() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    let started = call(
        &runtime,
        "repl_start",
        json!({
            "lang": "cat",
            "program": TEST_KERNEL.0,
            "args": TEST_KERNEL.1,
        }),
    )
    .await
    .expect("repl_start ok");
    let id = started["id"].as_str().unwrap().to_string();
    assert_eq!(started["lang"], "cat");
    assert!(!id.is_empty());

    let listed = call(&runtime, "repl_list", json!({}))
        .await
        .expect("repl_list ok");
    let entries = listed.as_array().expect("array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["id"], id);
    assert_eq!(entries[0]["lang"], "cat");
    assert_eq!(entries[0]["program"], TEST_KERNEL.0);

    call(&runtime, "repl_stop", json!({ "id": id }))
        .await
        .expect("repl_stop ok");

    let listed_after = call(&runtime, "repl_list", json!({}))
        .await
        .expect("repl_list ok");
    assert_eq!(listed_after.as_array().unwrap().len(), 0);
}

/// 2) Concurrent sessions — `repl_list` returns both with distinct ids.
#[cfg(unix)]
#[tokio::test]
async fn repl_concurrent_sessions_each_get_distinct_ids() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    let one = call(
        &runtime,
        "repl_start",
        json!({ "lang": "cat", "program": TEST_KERNEL.0, "args": TEST_KERNEL.1 }),
    )
    .await
    .expect("first start");
    let two = call(
        &runtime,
        "repl_start",
        json!({ "lang": "cat", "program": TEST_KERNEL.0, "args": TEST_KERNEL.1 }),
    )
    .await
    .expect("second start");
    assert_ne!(one["id"], two["id"]);

    let listed = call(&runtime, "repl_list", json!({}))
        .await
        .expect("repl_list ok");
    assert_eq!(listed.as_array().unwrap().len(), 2);
}

/// 3) Output streaming — eval a Python expression and observe its
///    output flow through the existing `read_raw_since` snapshot path
///    (which is the same byte stream the
///    `com.nexus.terminal.output.<id>` bus events carry). Skips with a
///    note when python3 is unavailable so the suite still passes on
///    minimal CI images.
#[cfg(unix)]
#[tokio::test]
async fn repl_eval_streams_python_print_output() {
    if !has_program("python3") {
        eprintln!("skipping: python3 not available");
        return;
    }

    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    let started = call(
        &runtime,
        "repl_start",
        json!({
            "lang": "python",
            "program": "python3",
            // -i forces interactive mode even when stdin isn't a tty
            // from the test driver's perspective; -q suppresses the
            // copyright banner so the post-eval output is easier to
            // scan for.
            "args": ["-iq"],
            // Force unbuffered stdout/stderr so `print()` output
            // reaches the PTY before the test polls for it. Python's
            // default block-buffering on a pipe-like stdio means a
            // short `print()` can sit in the kernel buffer for the
            // whole test window without `-u`.
            "env": [["PYTHONUNBUFFERED", "1"]],
        }),
    )
    .await
    .expect("repl_start python3");
    let id = started["id"].as_str().unwrap().to_string();

    call(
        &runtime,
        "repl_eval",
        json!({ "id": id, "code": "print(2+2)\n" }),
    )
    .await
    .expect("repl_eval ok");

    // Poll `read_raw_since` until we observe `4` in the byte stream
    // (or a 3 s budget elapses). Tight loop with sleeps keeps the
    // happy-path wall-clock under ~50 ms while still tolerating the
    // python startup + print round-trip on slower machines.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut cursor: u64 = 0;
    let mut acc = String::new();
    while std::time::Instant::now() < deadline {
        let resp = call(
            &runtime,
            "read_raw_since",
            json!({ "id": id, "cursor": cursor }),
        )
        .await
        .expect("read_raw_since ok");
        cursor = resp["cursor"].as_u64().unwrap_or(cursor);
        if let Some(arr) = resp["data"].as_array() {
            for b in arr {
                if let Some(byte) = b.as_u64() {
                    acc.push(byte as u8 as char);
                }
            }
        }
        if acc.contains('4') {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Best-effort shutdown so the test doesn't leak a python process.
    let _ = call(&runtime, "repl_stop", json!({ "id": id })).await;

    assert!(
        acc.contains('4'),
        "expected '4' in REPL output within 3 s; got: {acc:?}"
    );
}

/// 4) Read-only guard — `repl_eval` against a regular terminal session
///    (one created via `create_session` rather than `repl_start`)
///    rejects with a clear "not a registered REPL" message. Protects
///    against a keybinding mis-fire dropping code into the user's
///    shell.
#[cfg(unix)]
#[tokio::test]
async fn repl_eval_rejects_non_repl_session() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    // Create a regular terminal session via `create_session` (not
    // `repl_start`).
    let created = call(
        &runtime,
        "create_session",
        json!({
            "name": "regular",
            "shell": TEST_KERNEL.0,
            "shell_args": TEST_KERNEL.1,
        }),
    )
    .await
    .expect("create_session ok");
    let id = created["id"].as_str().unwrap().to_string();

    let err = call(
        &runtime,
        "repl_eval",
        json!({ "id": id, "code": "should not run\n" }),
    )
    .await
    .expect_err("repl_eval must reject non-REPL session");
    assert!(
        err.to_string().contains("not a registered REPL"),
        "expected 'not a registered REPL' in error, got: {err}"
    );

    // Also covers the stop-side guard.
    let stop_err = call(&runtime, "repl_stop", json!({ "id": id }))
        .await
        .expect_err("repl_stop must reject non-REPL session");
    assert!(
        stop_err.to_string().contains("not a registered REPL"),
        "expected 'not a registered REPL' in error, got: {stop_err}"
    );

    // Confirm the regular session is still alive (the rejected
    // `repl_stop` should NOT have torn it down).
    let listed = call(&runtime, "list_sessions", json!({}))
        .await
        .expect("list_sessions ok");
    let still_present = listed.as_array().unwrap().iter().any(|s| s["id"] == id);
    assert!(
        still_present,
        "rejected repl_stop must not tear down the regular session"
    );
}

/// 5) Error output — eval Python code that raises and observe the
///    error text on the same output stream. Confirms stderr reaches
///    the same PTY buffer (terminal sessions merge stderr into the
///    PTY by default, so error output streams alongside stdout
///    rather than via a separate channel).
#[cfg(unix)]
#[tokio::test]
async fn repl_eval_streams_python_error_output() {
    if !has_program("python3") {
        eprintln!("skipping: python3 not available");
        return;
    }

    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    let started = call(
        &runtime,
        "repl_start",
        json!({
            "lang": "python",
            "program": "python3",
            "args": ["-iq"],
            "env": [["PYTHONUNBUFFERED", "1"]],
        }),
    )
    .await
    .expect("repl_start python3");
    let id = started["id"].as_str().unwrap().to_string();

    // Reference an undefined name — Python emits a NameError to stderr.
    call(
        &runtime,
        "repl_eval",
        json!({ "id": id, "code": "this_name_does_not_exist\n" }),
    )
    .await
    .expect("repl_eval ok");

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut cursor: u64 = 0;
    let mut acc = String::new();
    while std::time::Instant::now() < deadline {
        let resp = call(
            &runtime,
            "read_raw_since",
            json!({ "id": id, "cursor": cursor }),
        )
        .await
        .expect("read_raw_since ok");
        cursor = resp["cursor"].as_u64().unwrap_or(cursor);
        if let Some(arr) = resp["data"].as_array() {
            for b in arr {
                if let Some(byte) = b.as_u64() {
                    acc.push(byte as u8 as char);
                }
            }
        }
        if acc.contains("NameError") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let _ = call(&runtime, "repl_stop", json!({ "id": id })).await;

    assert!(
        acc.contains("NameError"),
        "expected 'NameError' in REPL output within 3 s; got: {acc:?}"
    );
}

/// 6) Empty-list rejection isn't applicable to REPL handlers (the
///    DoD's "empty items" check belongs to BL-141 multibuffer);
///    instead, this test covers the stop-after-start hygiene case
///    that the BL DoD's "session cleaned up on tab close" UI test
///    implicitly relies on at the backend layer.
#[cfg(unix)]
#[tokio::test]
async fn repl_list_is_empty_before_any_start_and_after_stop() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    let initial = call(&runtime, "repl_list", json!({}))
        .await
        .expect("repl_list");
    assert_eq!(initial.as_array().unwrap().len(), 0);

    let started = call(
        &runtime,
        "repl_start",
        json!({ "lang": "cat", "program": TEST_KERNEL.0, "args": TEST_KERNEL.1 }),
    )
    .await
    .expect("repl_start");
    let id = started["id"].as_str().unwrap().to_string();
    call(&runtime, "repl_stop", json!({ "id": id }))
        .await
        .expect("repl_stop");

    let after = call(&runtime, "repl_list", json!({}))
        .await
        .expect("repl_list");
    assert_eq!(after.as_array().unwrap().len(), 0);
}
