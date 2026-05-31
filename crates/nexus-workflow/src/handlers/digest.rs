//! `com.nexus.workflow::run_digest` and `set_digest_config` async
//! handlers. Lifted out of `core_plugin.rs` by the BL-137 oversized-
//! file decomposition.

use std::sync::{Arc, RwLock};

use nexus_kernel::KernelPluginContext;
use nexus_plugins::PluginError;

use crate::digests;
use crate::DigestConfig;
use crate::DigestKind;

use super::shared::{exec_err, to_value};

pub(crate) async fn handle_run(
    ctx: Option<Arc<KernelPluginContext>>,
    cfg_handle: Arc<RwLock<DigestConfig>>,
    args: serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let ctx = ctx.ok_or_else(|| {
        exec_err("workflow plugin context not wired (bootstrap incomplete)".into())
    })?;
    let kind_str = args
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or_else(|| exec_err("run_digest: missing 'kind'".into()))?;
    let kind = DigestKind::from_str(kind_str).map_err(exec_err)?;
    // A5 (2026-05-21 audit) — same laundering tracker as workflow::run.
    // The digest pipeline is fixed: it always reads
    // `com.nexus.storage::query_files`/`read_file`, summarises via
    // `com.nexus.ai::ask` (requires `ai.chat`), and writes the
    // resulting markdown via `com.nexus.storage::write_file`. The
    // implied caller-cap surface is therefore constant: `ai.chat`.
    // Logging it here keeps `run_digest` consistent with
    // `workflow::run` for the audit-trail story (issue #77).
    tracing::warn!(
        audit = true,
        digest_kind = %kind_str,
        implied_caps = ?["ai.chat"],
        "workflow_run_digest: capability aggregation surface (issue #77 laundering tracker)"
    );
    let cfg = cfg_handle
        .read()
        .map(|g| g.clone())
        .map_err(|_| exec_err("run_digest: digest config lock poisoned".to_string()))?;
    let report = digests::run_digest(&ctx, &cfg, kind, chrono::Utc::now())
        .await
        .map_err(|e| exec_err(format!("run_digest: {e}")))?;
    to_value(&report, "run_digest")
}

/// FU-7 — `set_digest_config`: replace the live config under the
/// shared lock. The scheduler loop snapshots on every tick, so an
/// enabled-flip is picked up within 60 s.
///
/// Synchronous body wrapped at the call site as
/// `Box::pin(async move { handle_set_config(...) })` so the
/// `dispatch_async` signature still hands back a `CorePluginFuture`.
pub(crate) fn handle_set_config(
    cfg_handle: Arc<RwLock<DigestConfig>>,
    args: serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let new_cfg: DigestConfig = serde_json::from_value(args)
        .map_err(|e| exec_err(format!("set_digest_config: decode: {e}")))?;
    {
        let mut g = cfg_handle
            .write()
            .map_err(|_| exec_err("set_digest_config: digest config lock poisoned".to_string()))?;
        *g = new_cfg;
    }
    Ok(serde_json::json!({ "applied": true }))
}
