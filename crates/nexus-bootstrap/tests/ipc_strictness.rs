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
//! NOTE: a few subsystems (e.g. `com.nexus.storage`'s `read_file`,
//! `nexus-git`, `nexus-mcp`) bypass typed structs and read fields off
//! `serde_json::Value` directly via hand-rolled helpers. Those handlers
//! cannot be policed by this gate; they're tracked under issue #113
//! (wire remaining subsystems into the schema generator) and a planned
//! follow-up to refactor them to `parse_args::<TypedStruct>(...)`.

use std::time::Duration;

use nexus_bootstrap::build_cli_runtime;
use nexus_kernel::{Ipc as _, IpcError};

const CALL_TIMEOUT: Duration = Duration::from_secs(10);
const COMMENTS_PLUGIN_ID: &str = "com.nexus.comments";

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
        IpcError::PluginCrashedDuringCall { plugin_id, command, .. } => {
            assert_eq!(plugin_id, COMMENTS_PLUGIN_ID);
            assert_eq!(command, "list");
        }
        other => panic!(
            "expected PluginCrashedDuringCall on unknown field, got {other:?}"
        ),
    }
}
