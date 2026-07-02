//! C29 (#382) — `session_changes` / `session_revert` IPC handlers.
//!
//! Consume the write-snapshot trail the [`KernelToolBridge`] captures
//! during `session_run` (see [`crate::snapshots`]). `session_changes`
//! reports what a session touched (with hash context showing whether
//! the user edited since); `session_revert` restores the pre-session
//! content, skipping files whose current content no longer matches
//! what the session left behind unless `force` is passed.
//!
//! [`KernelToolBridge`]: crate::handlers::shared::KernelToolBridge

use std::sync::Arc;
use std::time::Duration;

use nexus_kernel::{Ipc as _, KernelPluginContext};
use nexus_plugins::PluginError;

use super::shared::exec_err;
use crate::snapshots::{self, RevertAction};

const IPC_TIMEOUT: Duration = Duration::from_secs(10);

fn session_id_arg(args: &serde_json::Value, verb: &str) -> Result<String, PluginError> {
    args.get("session_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| exec_err(format!("{verb}: missing 'session_id' string")))
}

async fn current_hash(ctx: &KernelPluginContext, path: &str) -> Option<String> {
    let reply: serde_json::Value = ctx
        .ipc_call(
            "com.nexus.storage",
            "read_file",
            serde_json::json!({ "path": path }),
            IPC_TIMEOUT,
        )
        .await
        .ok()?;
    let bytes: Vec<u8> = reply
        .get("bytes")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
        .collect();
    Some(snapshots::hash_hex(&bytes))
}

/// `session_changes { session_id }` → the session's write trail plus a
/// per-path `modified_since` flag (current content vs the hash the
/// session left behind).
pub(crate) async fn handle_session_changes(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let session_id = session_id_arg(args, "session_changes")?;
    let Some(trail) = snapshots::load_trail(&ctx, &session_id).await else {
        return Ok(serde_json::json!({ "session_id": session_id, "entries": [] }));
    };
    let mut entries = Vec::with_capacity(trail.entries.len());
    for e in &trail.entries {
        let modified_since = if e.path == "*" {
            serde_json::Value::Null
        } else {
            let last_post = trail
                .entries
                .iter()
                .rev()
                .find(|x| x.path == e.path)
                .and_then(|x| x.post_hash.clone());
            let current = current_hash(&ctx, &e.path).await;
            serde_json::Value::Bool(current != last_post)
        };
        entries.push(serde_json::json!({
            "seq": e.seq,
            "tool": e.tool,
            "path": e.path,
            "had_prior": e.prior_b64.is_some(),
            "prior_hash": e.prior_hash,
            "post_hash": e.post_hash,
            "captured_at_ms": e.captured_at_ms,
            "modified_since": modified_since,
        }));
    }
    Ok(serde_json::json!({ "session_id": session_id, "entries": entries }))
}

/// `session_revert { session_id, paths?, force? }` → restore the
/// pre-session content. Per-path outcomes:
///   - `restored` — prior bytes written back,
///   - `removed`  — session-created file deleted,
///   - `skipped`  — user edited the file after the session (pass
///     `force: true` to override), or the path filter excluded it.
pub(crate) async fn handle_session_revert(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let session_id = session_id_arg(args, "session_revert")?;
    let force = args
        .get("force")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let path_filter: Option<Vec<String>> = args.get("paths").and_then(|v| {
        v.as_array().map(|arr| {
            arr.iter()
                .filter_map(|p| p.as_str().map(str::to_string))
                .collect()
        })
    });

    let Some(trail) = snapshots::load_trail(&ctx, &session_id).await else {
        return Ok(serde_json::json!({
            "session_id": session_id,
            "results": [],
            "note": "no snapshot trail recorded for this session",
        }));
    };

    let mut results = Vec::new();
    for (action, expected_current) in snapshots::revert_plan(&trail.entries) {
        let path = match &action {
            RevertAction::Restore { path, .. } | RevertAction::Remove { path } => path.clone(),
        };
        if let Some(filter) = &path_filter {
            if !filter.iter().any(|p| p == &path) {
                continue;
            }
        }
        // Guard: only revert what the session left behind. A mismatch
        // means the user (or another process) edited since.
        if !force {
            let current = current_hash(&ctx, &path).await;
            if current != expected_current {
                results.push(serde_json::json!({
                    "path": path,
                    "action": "skipped",
                    "reason": "modified since the session ended (pass force to override)",
                }));
                continue;
            }
        }
        match action {
            RevertAction::Restore { path, prior_b64 } => {
                let Some(bytes) = snapshots::decode_prior(&prior_b64) else {
                    results.push(serde_json::json!({
                        "path": path,
                        "action": "skipped",
                        "reason": "snapshot payload corrupt",
                    }));
                    continue;
                };
                match ctx
                    .ipc_call(
                        "com.nexus.storage",
                        "write_file",
                        serde_json::json!({ "path": &path, "bytes": bytes }),
                        IPC_TIMEOUT,
                    )
                    .await
                {
                    Ok(_) => results.push(serde_json::json!({
                        "path": path, "action": "restored",
                    })),
                    Err(e) => results.push(serde_json::json!({
                        "path": path, "action": "skipped",
                        "reason": format!("restore failed: {e}"),
                    })),
                }
            }
            RevertAction::Remove { path } => {
                match ctx
                    .ipc_call(
                        "com.nexus.storage",
                        "trash_entry",
                        serde_json::json!({ "relpath": &path, "destination": "forge" }),
                        IPC_TIMEOUT,
                    )
                    .await
                {
                    Ok(_) => results.push(serde_json::json!({
                        "path": path, "action": "removed",
                    })),
                    Err(e) => results.push(serde_json::json!({
                        "path": path, "action": "skipped",
                        "reason": format!("remove failed: {e}"),
                    })),
                }
            }
        }
    }
    Ok(serde_json::json!({ "session_id": session_id, "results": results }))
}
