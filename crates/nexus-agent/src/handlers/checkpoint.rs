//! Session-checkpoint handlers (RFC 0008 / Phase 5.4).
//!
//! A checkpoint is a named pointer at a `(session_id, round)` location. Under
//! the immutable-fork session model that coordinate already *is* a snapshot, so
//! a checkpoint stores no transcript — just a stable, human-friendly handle a
//! user can later list and branch from. The whole set lives in one JSON array
//! at `<forge>/.forge/agent/sessions/checkpoints.json`.

use std::sync::Arc;
use std::time::Duration;

use nexus_kernel::{FileSystem as _, Ipc as _, KernelPluginContext};
use nexus_plugins::PluginError;
use serde::Deserialize;

use crate::session::SessionCheckpoint;

use super::session::{assemble_session, resolve_fork_point, SESSION_DIR};
use super::shared::{exec_err, parse_args};

/// Sibling of the per-session transcript files; one array for the whole forge.
fn checkpoints_path() -> std::path::PathBuf {
    std::path::PathBuf::from(format!("{SESSION_DIR}/checkpoints.json"))
}

#[derive(Debug, Deserialize)]
struct CheckpointArgs {
    session_id: String,
    round: u32,
    name: String,
}

#[derive(Debug, Deserialize)]
struct CheckpointNameArgs {
    name: String,
}

/// `session_checkpoint { session_id, round, name }` — name a `(session, round)`
/// location. Validates the coordinate exists (assembling the session and
/// range-checking the round) before recording it, then upserts by name.
pub(crate) async fn handle_session_checkpoint(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: CheckpointArgs = parse_args(args, "session_checkpoint")?;
    let name = a.name.trim().to_string();
    if name.is_empty() {
        return Err(exec_err("session_checkpoint: `name` must be non-empty".into()));
    }
    // Validate the (session, round) coordinate before bookmarking it.
    let parent = assemble_session(&ctx, a.session_id.clone(), 0).await?;
    let tip = parent.rounds.last().map_or(0, |r| r.round);
    resolve_fork_point(Some(a.round), tip)
        .map_err(|e| exec_err(format!("session_checkpoint: {e}")))?;

    let cp = SessionCheckpoint {
        name,
        session_id: a.session_id,
        round: a.round,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let mut list = load_checkpoints(&ctx).await;
    upsert_checkpoint(&mut list, cp.clone());
    save_checkpoints(&ctx, &list).await?;
    serde_json::to_value(&cp).map_err(|e| exec_err(format!("session_checkpoint: encode: {e}")))
}

/// `session_checkpoints` — list every checkpoint (most-recent-first).
pub(crate) async fn handle_session_checkpoints(
    ctx: Arc<KernelPluginContext>,
) -> Result<serde_json::Value, PluginError> {
    let list = load_checkpoints(&ctx).await;
    serde_json::to_value(&list).map_err(|e| exec_err(format!("session_checkpoints: encode: {e}")))
}

/// `session_checkpoint_delete { name }` — remove a checkpoint by name.
pub(crate) async fn handle_session_checkpoint_delete(
    ctx: Arc<KernelPluginContext>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: CheckpointNameArgs = parse_args(args, "session_checkpoint_delete")?;
    let mut list = load_checkpoints(&ctx).await;
    let before = list.len();
    list.retain(|c| c.name != a.name);
    let deleted = before != list.len();
    if deleted {
        save_checkpoints(&ctx, &list).await?;
    }
    Ok(serde_json::json!({ "deleted": deleted, "name": a.name }))
}

/// Upsert by name: drop any existing checkpoint sharing `cp`'s name, then push
/// `cp` to the front so the list reads most-recent-first. Pure.
fn upsert_checkpoint(list: &mut Vec<SessionCheckpoint>, cp: SessionCheckpoint) {
    list.retain(|c| c.name != cp.name);
    list.insert(0, cp);
}

/// Load the checkpoint array. A missing or malformed file reads as empty — the
/// file is derived, human-editable metadata, never the source of truth.
async fn load_checkpoints(ctx: &KernelPluginContext) -> Vec<SessionCheckpoint> {
    let Ok(bytes) = ctx.read_file(&checkpoints_path()).await else {
        return Vec::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// Persist the checkpoint array via the storage vault-write path.
async fn save_checkpoints(
    ctx: &KernelPluginContext,
    list: &[SessionCheckpoint],
) -> Result<(), PluginError> {
    let path = checkpoints_path();
    let bytes = serde_json::to_vec_pretty(list)
        .map_err(|e| exec_err(format!("session_checkpoint: encode list: {e}")))?;
    let path_str = path
        .to_str()
        .ok_or_else(|| exec_err("session_checkpoint: path not UTF-8".into()))?;
    ctx.ipc_call(
        "com.nexus.storage",
        "write_vault_file",
        serde_json::json!({ "path": path_str, "bytes": bytes }),
        Duration::from_secs(10),
    )
    .await
    .map_err(|e| exec_err(format!("session_checkpoint: persist: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cp(name: &str, round: u32) -> SessionCheckpoint {
        SessionCheckpoint {
            name: name.into(),
            session_id: "s".into(),
            round,
            created_at: "2026-06-17T00:00:00Z".into(),
        }
    }

    #[test]
    fn upsert_adds_new_to_front() {
        let mut list = vec![cp("a", 1)];
        upsert_checkpoint(&mut list, cp("b", 2));
        assert_eq!(
            list.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            vec!["b", "a"]
        );
    }

    #[test]
    fn upsert_replaces_same_name_in_place_of_a_duplicate() {
        let mut list = vec![cp("a", 1), cp("b", 2)];
        // Re-checkpointing "a" at a new round replaces the old one (no dup) and
        // moves it to the front.
        upsert_checkpoint(&mut list, cp("a", 9));
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "a");
        assert_eq!(list[0].round, 9);
        assert_eq!(list.iter().filter(|c| c.name == "a").count(), 1);
    }
}
