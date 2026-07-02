//! C29 (#382) — session write-snapshots + revert.
//!
//! Every storage write an agent session performs through the
//! [`KernelToolBridge`](crate::handlers::shared) captures the target
//! file's **pre-write bytes** (and, after the call, the post-write
//! content hash) into `.forge/agent/snapshots/<session_id>.json`.
//! Two IPC verbs consume the trail:
//!
//!   - `session_changes` — "what did this session touch?", with
//!     enough hash context to see whether the user edited since;
//!   - `session_revert` — restore the pre-session content (first
//!     snapshot per path wins), guarded so a file the user has edited
//!     after the session is skipped unless `force` is passed.
//!
//! This deliberately lives on the agent's own dispatch chokepoint
//! rather than inside `nexus-storage`: only agent-authored writes are
//! snapshotted, and the trail is scoped/keyed by session. Bulk verbs
//! whose target set isn't knowable before dispatch
//! (`replace_in_files`) are recorded as unsnapshotted markers so the
//! changes view stays honest about its blind spots.

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

/// Where per-session snapshot trails live (derived state, not indexed).
pub const SNAPSHOT_DIR: &str = ".forge/agent/snapshots";

/// Storage verbs whose pre-state we can capture from their args.
/// `(command_id, args key holding the forge-relative path)`.
const SNAPSHOT_VERBS: &[(&str, &str)] = &[
    ("write_file", "path"),
    ("write_frontmatter", "path"),
    ("note_append", "path"),
    ("delete_file", "path"),
    ("delete_entry", "relpath"),
    ("trash_entry", "relpath"),
];

/// Bulk verbs we cannot snapshot pre-dispatch — recorded as markers.
const BULK_VERBS: &[&str] = &["replace_in_files"];

/// One captured write.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEntry {
    /// Monotonic order within the session's trail.
    pub seq: u64,
    /// Storage command that performed the write.
    pub tool: String,
    /// Forge-relative path. `"*"` for bulk markers ([`BULK_VERBS`]).
    pub path: String,
    /// Base64 of the pre-write bytes; `None` when the file did not
    /// exist before the call (or for bulk markers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_b64: Option<String>,
    /// SHA-256 hex of the pre-write bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_hash: Option<String>,
    /// SHA-256 hex of the content right after the call; `None` when
    /// the call deleted the file (or for bulk markers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_hash: Option<String>,
    /// Unix epoch ms at capture time.
    pub captured_at_ms: u64,
}

/// The whole trail for one session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionSnapshots {
    /// The agent session this trail belongs to.
    pub session_id: String,
    /// Captured writes, in dispatch order.
    #[serde(default)]
    pub entries: Vec<SnapshotEntry>,
}

/// SHA-256 hex of `bytes`.
#[must_use]
pub fn hash_hex(bytes: &[u8]) -> String {
    let mut h = sha2::Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Base64-encode pre-write bytes for JSON storage.
#[must_use]
pub fn encode_prior(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Decode [`encode_prior`] output; `None` on corrupt data.
#[must_use]
pub fn decode_prior(b64: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(b64).ok()
}

/// Forge-relative path of a session's snapshot trail. `None` for ids
/// that are empty or path-shaped (defense against traversal).
#[must_use]
pub fn trail_path(session_id: &str) -> Option<String> {
    if session_id.is_empty()
        || !session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    Some(format!("{SNAPSHOT_DIR}/{session_id}.json"))
}

/// Paths a storage call will write, extracted from its args before
/// dispatch. Empty for non-write verbs. The hashline `edit` verb
/// carries its targets in `[PATH#TAG]` section headers inside the
/// patch text.
#[must_use]
pub fn write_paths_for(command_id: &str, args: &serde_json::Value) -> Vec<String> {
    for (verb, key) in SNAPSHOT_VERBS {
        if command_id == *verb {
            return args
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(|p| vec![p.to_string()])
                .unwrap_or_default();
        }
    }
    if command_id == "edit" {
        return args
            .get("patch")
            .and_then(serde_json::Value::as_str)
            .map(patch_paths)
            .unwrap_or_default();
    }
    Vec::new()
}

/// `true` for write verbs whose target set is unknowable pre-dispatch.
#[must_use]
pub fn is_bulk_write(command_id: &str) -> bool {
    BULK_VERBS.contains(&command_id)
}

/// Extract the distinct paths from a hashline patch's `[PATH#TAG]`
/// section headers (RFC 0005). Tolerates leading whitespace; ignores
/// non-header lines.
#[must_use]
pub fn patch_paths(patch: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in patch.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let Some(inner) = rest.strip_suffix(']') else {
            continue;
        };
        let path = inner.rsplit_once('#').map_or(inner, |(p, _)| p).trim();
        if !path.is_empty() && !out.iter().any(|p| p == path) {
            out.push(path.to_string());
        }
    }
    out
}

/// A planned revert action for one path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevertAction {
    /// Write these prior bytes back.
    Restore {
        /// Forge-relative path to restore.
        path: String,
        /// Base64 pre-session bytes to write back.
        prior_b64: String,
    },
    /// The file didn't exist pre-session — delete it.
    Remove {
        /// Forge-relative path to remove.
        path: String,
    },
}

/// Fold a trail into per-path revert actions: the **first** snapshot
/// per path wins (that's the pre-session state), bulk markers are
/// skipped, and the expected current hash is the **last** entry's
/// `post_hash` (what the session left behind).
#[must_use]
pub fn revert_plan(entries: &[SnapshotEntry]) -> Vec<(RevertAction, Option<String>)> {
    let mut out: Vec<(RevertAction, Option<String>)> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for entry in entries {
        if entry.path == "*" || seen.iter().any(|p| *p == entry.path) {
            continue;
        }
        seen.push(&entry.path);
        let expected_current = entries
            .iter()
            .rev()
            .find(|e| e.path == entry.path)
            .and_then(|e| e.post_hash.clone());
        let action = match &entry.prior_b64 {
            Some(b64) => RevertAction::Restore {
                path: entry.path.clone(),
                prior_b64: b64.clone(),
            },
            None => RevertAction::Remove {
                path: entry.path.clone(),
            },
        };
        out.push((action, expected_current));
    }
    out
}

// ── ctx-based IO (storage IPC) ───────────────────────────────────────

use std::time::Duration;

use nexus_kernel::{Ipc as _, KernelPluginContext};

const IPC_TIMEOUT: Duration = Duration::from_secs(10);

/// One pre-dispatch capture: the target path and its pre-write bytes
/// (`None` = file absent).
pub(crate) struct PendingCapture {
    pub(crate) path: String,
    pub(crate) prior: Option<Vec<u8>>,
}

async fn read_bytes(ctx: &KernelPluginContext, path: &str) -> Option<Vec<u8>> {
    let reply: Result<serde_json::Value, _> = ctx
        .ipc_call(
            "com.nexus.storage",
            "read_file",
            serde_json::json!({ "path": path }),
            IPC_TIMEOUT,
        )
        .await;
    reply
        .ok()?
        .get("bytes")?
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
                .collect()
        })
}

/// Pre-dispatch: capture prior bytes for every path the call will
/// write. Empty for non-write / unknown verbs. Best-effort — a failed
/// read is recorded as "absent" rather than blocking the tool call.
pub(crate) async fn capture_before(
    ctx: &KernelPluginContext,
    command_id: &str,
    args: &serde_json::Value,
) -> Vec<PendingCapture> {
    let mut out = Vec::new();
    for path in write_paths_for(command_id, args) {
        let prior = read_bytes(ctx, &path).await;
        out.push(PendingCapture { path, prior });
    }
    if out.is_empty() && is_bulk_write(command_id) {
        out.push(PendingCapture {
            path: "*".to_string(),
            prior: None,
        });
    }
    out
}

/// Post-dispatch (success only): hash the paths' current content,
/// append entries to the session trail, persist it. Best-effort — a
/// snapshot failure must never fail the tool call that already ran.
pub(crate) async fn commit_after(
    ctx: &KernelPluginContext,
    session_id: &str,
    tool: &str,
    pending: Vec<PendingCapture>,
) {
    if pending.is_empty() {
        return;
    }
    let Some(trail_relpath) = trail_path(session_id) else {
        return;
    };
    let mut trail = match read_bytes(ctx, &trail_relpath).await {
        Some(bytes) => serde_json::from_slice::<SessionSnapshots>(&bytes).unwrap_or_default(),
        None => SessionSnapshots::default(),
    };
    trail.session_id = session_id.to_string();
    let mut seq = trail.entries.last().map_or(0, |e| e.seq);
    let now = crate::handlers::shared::now_unix_ms();
    for capture in pending {
        seq += 1;
        let post_hash = if capture.path == "*" {
            None
        } else {
            read_bytes(ctx, &capture.path).await.map(|b| hash_hex(&b))
        };
        trail.entries.push(SnapshotEntry {
            seq,
            tool: tool.to_string(),
            path: capture.path,
            prior_b64: capture.prior.as_deref().map(encode_prior),
            prior_hash: capture.prior.as_deref().map(hash_hex),
            post_hash,
            captured_at_ms: now,
        });
    }
    let Ok(bytes) = serde_json::to_vec_pretty(&trail) else {
        return;
    };
    let _ = ctx
        .ipc_call(
            "com.nexus.storage",
            "write_vault_file",
            serde_json::json!({ "path": trail_relpath, "bytes": bytes }),
            IPC_TIMEOUT,
        )
        .await
        .map_err(|e| {
            tracing::warn!(session_id, error = %e, "C29: snapshot trail persist failed");
        });
}

/// Load a session's trail (empty when none was recorded).
pub(crate) async fn load_trail(
    ctx: &KernelPluginContext,
    session_id: &str,
) -> Option<SessionSnapshots> {
    let trail_relpath = trail_path(session_id)?;
    let bytes = read_bytes(ctx, &trail_relpath).await?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(seq: u64, path: &str, prior: Option<&str>, post: Option<&str>) -> SnapshotEntry {
        SnapshotEntry {
            seq,
            tool: "write_file".into(),
            path: path.into(),
            prior_b64: prior.map(|s| encode_prior(s.as_bytes())),
            prior_hash: prior.map(|s| hash_hex(s.as_bytes())),
            post_hash: post.map(|s| hash_hex(s.as_bytes())),
            captured_at_ms: seq,
        }
    }

    #[test]
    fn write_paths_extraction_per_verb() {
        assert_eq!(
            write_paths_for("write_file", &serde_json::json!({"path": "a.md"})),
            vec!["a.md"]
        );
        assert_eq!(
            write_paths_for("delete_entry", &serde_json::json!({"relpath": "dir/b.md"})),
            vec!["dir/b.md"]
        );
        assert!(write_paths_for("read_file", &serde_json::json!({"path": "a.md"})).is_empty());
        assert!(write_paths_for("write_file", &serde_json::json!({})).is_empty());
    }

    #[test]
    fn edit_patch_paths_parse_section_headers() {
        let patch = "[notes/a.md#1A2B]\nSWAP 1.=1:\n+new\n[notes/b.md#FFFF]\n+tail\n[notes/a.md#1A2B]\nDEL 2.=2";
        assert_eq!(
            write_paths_for("edit", &serde_json::json!({"patch": patch})),
            vec!["notes/a.md", "notes/b.md"]
        );
    }

    #[test]
    fn trail_path_rejects_path_shaped_ids() {
        assert!(trail_path("../evil").is_none());
        assert!(trail_path("").is_none());
        assert!(trail_path("sess-01_a").is_some());
    }

    #[test]
    fn revert_plan_first_snapshot_wins_and_carries_last_post_hash() {
        let entries = vec![
            entry(1, "a.md", Some("original"), Some("v1")),
            entry(2, "a.md", Some("v1"), Some("v2")),
            entry(3, "new.md", None, Some("fresh")),
        ];
        let plan = revert_plan(&entries);
        assert_eq!(plan.len(), 2);
        match &plan[0] {
            (RevertAction::Restore { path, prior_b64 }, expected) => {
                assert_eq!(path, "a.md");
                assert_eq!(decode_prior(prior_b64).unwrap(), b"original");
                assert_eq!(expected.as_deref(), Some(hash_hex(b"v2").as_str()));
            }
            other => panic!("unexpected: {other:?}"),
        }
        match &plan[1] {
            (RevertAction::Remove { path }, expected) => {
                assert_eq!(path, "new.md");
                assert_eq!(expected.as_deref(), Some(hash_hex(b"fresh").as_str()));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn bulk_markers_are_skipped_by_the_plan() {
        let mut marker = entry(1, "*", None, None);
        marker.tool = "replace_in_files".into();
        assert!(revert_plan(&[marker]).is_empty());
        assert!(is_bulk_write("replace_in_files"));
    }
}
