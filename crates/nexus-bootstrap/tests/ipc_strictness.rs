//! Audit-2026-05-01 P0-1: workspace IPC strictness sanity test.
//!
//! Proves that an unknown field on an IPC payload is rejected end-to-end
//! through the kernel dispatcher rather than silently round-tripping.
//! Without this gate, a future struct could lose its
//! `#[serde(deny_unknown_fields)]` and the regression would only surface
//! as silent contract drift.
//!
//! The companion P0-2 test is in `ipc_schema_emit.rs` and polices the
//! generated JSON schemas. This test polices the live dispatcher — they
//! catch different classes of regression.
//!
//! Targets `com.nexus.comments::list`. That handler deserializes args
//! through the typed `FilePathArg` (now `deny_unknown_fields`) before
//! calling into the comment store, so an extra field hits the strict
//! deser path and surfaces as `PluginCrashedDuringCall`.
//!
//! NOTE: a few subsystems (`nexus-mcp`, `nexus-lsp`, `nexus-dap`
//! reply paths) still bypass typed structs and read fields off
//! `serde_json::Value` directly via hand-rolled helpers. Those
//! handlers cannot be policed by this gate; they're tracked under
//! issue #190 and follow-ups to refactor them to
//! `parse_args::<TypedStruct>(...)`.
//!
//! `com.nexus.storage::read_file` (previously on the bypassed list)
//! migrated to `StorageReadFileArgs` / `StorageReadFileResult` in PR
//! #212. The `nexus-git` branch handlers (`switch_branch`,
//! `create_branch`, `delete_branch`, `push`) migrated in the next PR
//! against #190 to `GitBranchArgs` / `GitPushArgs` / `GitOk`. Both
//! are now policed below.

use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_kernel::{Ipc as _, IpcError};

const CALL_TIMEOUT: Duration = Duration::from_secs(10);
const COMMENTS_PLUGIN_ID: &str = "com.nexus.comments";
const STORAGE_PLUGIN_ID: &str = "com.nexus.storage";
const GIT_PLUGIN_ID: &str = "com.nexus.git";
const MCP_PLUGIN_ID: &str = "com.nexus.mcp.host";

fn scratch_forge() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init scratch forge");
    dir
}

#[tokio::test]
async fn comments_list_rejects_payload_with_unknown_field() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    // Baseline: the strict shape must still succeed end-to-end so the
    // failure below cannot be attributed to a misconfigured plugin.
    runtime
        .context
        .ipc_call(
            COMMENTS_PLUGIN_ID,
            "list",
            serde_json::json!({ "file_path": "notes/foo.md" }),
            CALL_TIMEOUT,
        )
        .await
        .expect("baseline list with strict args succeeds");

    // Pre-P0-1 the typo `file_pathh` would round-trip silently because
    // FilePathArg ignored unknown fields and `file_path` defaulted to
    // an empty string, returning `[]`. Post-P0-1 the strict deser
    // surfaces the typo as PluginCrashedDuringCall.
    let err = runtime
        .context
        .ipc_call(
            COMMENTS_PLUGIN_ID,
            "list",
            serde_json::json!({
                "file_path": "notes/foo.md",
                "file_pathh": "typo",
            }),
            CALL_TIMEOUT,
        )
        .await
        .expect_err("unknown field must be rejected");

    match err {
        IpcError::PluginCrashedDuringCall {
            plugin_id, command, ..
        } => {
            assert_eq!(plugin_id, COMMENTS_PLUGIN_ID);
            assert_eq!(command, "list");
        }
        other => panic!("expected PluginCrashedDuringCall on unknown field, got {other:?}"),
    }
}

/// #190 / R7 — `com.nexus.storage::read_file` was the audit's named
/// example of a hand-rolled `serde_json::Value` reader that bypassed
/// the strictness gate. PR #(this one) migrated it to typed
/// `StorageReadFileArgs` / `StorageReadFileResult`, so the unknown-
/// field rejection now applies end-to-end. This test locks the new
/// contract so a future refactor that drops `deny_unknown_fields` or
/// reverts to a hand-rolled reader fails CI rather than silently
/// re-opening the drift surface.
#[tokio::test]
async fn storage_read_file_rejects_unknown_field() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    // Baseline: the strict shape returns the typed `{ bytes: null }`
    // result for a missing file. Using a path that doesn't exist
    // exercises the FileNotFound branch without seeding any state.
    runtime
        .context
        .ipc_call(
            STORAGE_PLUGIN_ID,
            "read_file",
            serde_json::json!({ "path": "does/not/exist.md" }),
            CALL_TIMEOUT,
        )
        .await
        .expect("baseline read_file with strict args succeeds");

    let err = runtime
        .context
        .ipc_call(
            STORAGE_PLUGIN_ID,
            "read_file",
            serde_json::json!({
                "path": "does/not/exist.md",
                "pathh": "typo",
            }),
            CALL_TIMEOUT,
        )
        .await
        .expect_err("unknown field must be rejected");

    match err {
        IpcError::PluginCrashedDuringCall {
            plugin_id, command, ..
        } => {
            assert_eq!(plugin_id, STORAGE_PLUGIN_ID);
            assert_eq!(command, "read_file");
        }
        other => panic!("expected PluginCrashedDuringCall on unknown field, got {other:?}"),
    }
}

/// #190 / R7 — `com.nexus.git::switch_branch` was previously a
/// hand-rolled `key_string` + `json!({"ok": true})` handler. It
/// migrated to typed `GitBranchArgs` / `GitOk`, both
/// `deny_unknown_fields`. This test fires `{ name, namee }` against
/// the live runtime and asserts the strict deser rejects the typo.
/// Targets `switch_branch` (vs. `create_branch` / `delete_branch` /
/// `push`) because all four sit on the same `GitBranchArgs`-shaped
/// parse path — one assertion covers the family.
#[tokio::test]
async fn git_switch_branch_rejects_unknown_field() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    // No baseline call — `switch_branch("main")` against a scratch
    // forge with no `.git/` would error inside the worker for an
    // unrelated reason (no repo). The unknown-field rejection runs
    // *before* the handler body, so this still proves the strict
    // gate fires.
    let err = runtime
        .context
        .ipc_call(
            GIT_PLUGIN_ID,
            "switch_branch",
            serde_json::json!({
                "name": "main",
                "namee": "typo",
            }),
            CALL_TIMEOUT,
        )
        .await
        .expect_err("unknown field must be rejected");

    match err {
        IpcError::PluginCrashedDuringCall {
            plugin_id, command, ..
        } => {
            assert_eq!(plugin_id, GIT_PLUGIN_ID);
            assert_eq!(command, "switch_branch");
        }
        other => panic!("expected PluginCrashedDuringCall on unknown field, got {other:?}"),
    }
}

/// #190 / R7 — `com.nexus.mcp.host::unregister_tool` was previously a
/// hand-rolled `str_arg(args, "name")` lookup that ignored unknown
/// fields. It migrated to typed `McpUnregisterToolArgs` /
/// `McpUnregisterToolReply`. This test fires `{ name, namee }` and
/// asserts the strict deser rejects the typo. Sync handler — no
/// runtime to spin up beyond the standard scratch forge.
#[tokio::test]
async fn mcp_unregister_tool_rejects_unknown_field() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("runtime");

    // Baseline: the strict shape returns `{ removed: false, name }`
    // when no such registration exists. (No prior `register_tool`
    // call, so the unregister is a no-op success.)
    runtime
        .context
        .ipc_call(
            MCP_PLUGIN_ID,
            "unregister_tool",
            serde_json::json!({ "name": "no-such-tool" }),
            CALL_TIMEOUT,
        )
        .await
        .expect("baseline unregister_tool with strict args succeeds");

    let err = runtime
        .context
        .ipc_call(
            MCP_PLUGIN_ID,
            "unregister_tool",
            serde_json::json!({
                "name": "no-such-tool",
                "namee": "typo",
            }),
            CALL_TIMEOUT,
        )
        .await
        .expect_err("unknown field must be rejected");

    match err {
        IpcError::PluginCrashedDuringCall {
            plugin_id, command, ..
        } => {
            assert_eq!(plugin_id, MCP_PLUGIN_ID);
            assert_eq!(command, "unregister_tool");
        }
        other => panic!("expected PluginCrashedDuringCall on unknown field, got {other:?}"),
    }
}
