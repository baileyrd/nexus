//! BL-129 Dream Cycle scheduler — fires the four maintenance phases
//! on the configured cron schedule.
//!
//! Long-running invokers (TUI, shell, MCP) call [`spawn`] after
//! constructing their [`Runtime`]; the function returns a
//! [`DreamCycleScheduler`] whose drop / explicit `stop` joins the
//! background thread. CLI consumers skip the scheduler entirely —
//! `nexus graph dream-cycle run` already covers on-demand execution
//! and a one-shot binary has nothing to schedule.
//!
//! # Configuration
//!
//! Read from `<forge>/.forge/app.toml` `[dream_cycle]` block — see
//! [`nexus_formats::config::DreamCycleSettings`]. When `enabled` is
//! `false`, the loop polls the config at a 60-second cadence so a
//! `nexus config edit` flip takes effect within a minute without
//! requiring a restart.
//!
//! # Cron evaluation
//!
//! Uses [`nexus_workflow::CronSchedule`] (the same parser the workflow
//! engine drives) so the schedule expression is interpreted
//! identically to user-defined workflows. Tick cadence is bounded by
//! `MIN_SLEEP_SECS` so a malformed schedule that resolves to a
//! sub-second next-fire doesn't hot-spin.
//!
//! # Phases fired per cycle
//!
//! In spec order: `dedup` → `decay` → `enrich` → `infer`. Each phase
//! invokes the dedicated IPC handlers (`com.nexus.storage::*` for the
//! deterministic phases, `com.nexus.ai::*` for the LLM phases). The
//! LLM phases short-circuit when no AI provider is configured, so a
//! forge without AI still gets the dedup + decay benefits.

#![allow(
    clippy::needless_pass_by_value,    // PathBuf / Arc / Receiver are moved into the worker thread
    clippy::too_many_lines,            // run_cycle is sequential phases, intentional shape
    clippy::cast_possible_truncation,  // elapsed().as_millis() fits u64 for any reasonable cycle
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::similar_names,
)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::Utc;
use nexus_kernel::KernelPluginContext;
use nexus_workflow::CronSchedule;

use crate::{all_caps, Runtime};

const SCHEDULER_PLUGIN_ID: &str = "com.nexus.dream_cycle.scheduler";
const SCHEDULER_PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");
const IPC_TIMEOUT: Duration = Duration::from_secs(120);
/// Floor on how long we sleep between cron evaluations — prevents a
/// pathological schedule from busy-looping.
const MIN_SLEEP_SECS: u64 = 5;
/// Cadence for re-checking `[dream_cycle].enabled` when the scheduler
/// is currently disabled or the schedule failed to parse.
const DISABLED_POLL_SECS: u64 = 60;

/// Handle returned by [`spawn`]. Dropping the handle signals stop and
/// joins the worker thread. Callers that want a graceful shutdown
/// (TUI exit, shell tab close) can call [`DreamCycleScheduler::stop`]
/// explicitly to surface any join error.
pub struct DreamCycleScheduler {
    stop_tx: mpsc::SyncSender<()>,
    stopped: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl DreamCycleScheduler {
    /// Signal the worker to stop and wait for it. Idempotent.
    pub fn stop(mut self) {
        let _ = self.stop_tx.try_send(());
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
    }
}

impl Drop for DreamCycleScheduler {
    fn drop(&mut self) {
        let _ = self.stop_tx.try_send(());
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
    }
}

/// Spawn the dream-cycle scheduler. The returned handle ties the
/// worker thread's lifetime to the caller — drop it to stop, or call
/// [`DreamCycleScheduler::stop`] explicitly.
///
/// The function fails only when a fresh [`KernelPluginContext`]
/// cannot be built (typically: the forge root can't be canonicalised).
/// In every other failure mode the worker thread logs a `tracing`
/// warning and continues, so a transient app.toml parse error does
/// not kill the scheduler.
///
/// # Errors
/// Returns the kernel error surfaced by [`KernelPluginContext::new`].
pub fn spawn(runtime: &Runtime, forge_root: PathBuf) -> Result<DreamCycleScheduler> {
    let ctx = KernelPluginContext::new(
        SCHEDULER_PLUGIN_ID,
        SCHEDULER_PLUGIN_VERSION,
        all_caps(),
        runtime.kernel.kv_store(),
        runtime.kernel.event_bus(),
        &forge_root,
        Some(Arc::clone(&runtime.loader) as Arc<dyn nexus_kernel::IpcDispatcher>),
    )
    .map_err(|e| anyhow::anyhow!("dream_cycle: build scheduler context: {e}"))?
    .with_trust_level(nexus_kernel::TrustLevel::Core);
    let ctx = Arc::new(ctx);

    let (stop_tx, stop_rx) = mpsc::sync_channel::<()>(1);
    let stopped = Arc::new(AtomicBool::new(false));
    let stopped_clone = Arc::clone(&stopped);
    let forge_root_clone = forge_root.clone();

    let thread = std::thread::Builder::new()
        .name("nexus-dream-cycle".to_string())
        .spawn(move || {
            run_loop(&ctx, &forge_root_clone, stop_rx, stopped_clone);
        })
        .map_err(|e| anyhow::anyhow!("dream_cycle: spawn scheduler thread: {e}"))?;

    Ok(DreamCycleScheduler {
        stop_tx,
        stopped,
        thread: Some(thread),
    })
}

fn run_loop(
    ctx: &Arc<KernelPluginContext>,
    forge_root: &std::path::Path,
    stop_rx: mpsc::Receiver<()>,
    stopped: Arc<AtomicBool>,
) {
    // One current-thread tokio runtime per worker — every IPC call is
    // driven through `rt.block_on(...)`. Per-call new-runtime would be
    // simpler but wastes ~ms of setup; a worker-scoped one is cheap.
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            tracing::warn!(error = %e, "dream_cycle: failed to build tokio runtime; scheduler disabled");
            return;
        }
    };

    loop {
        if stopped.load(Ordering::SeqCst) {
            return;
        }
        let cfg = match load_settings(forge_root) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, "dream_cycle: app.toml read failed; sleeping {DISABLED_POLL_SECS}s");
                if wait(&stop_rx, Duration::from_secs(DISABLED_POLL_SECS)) {
                    return;
                }
                continue;
            }
        };
        if !cfg.enabled {
            if wait(&stop_rx, Duration::from_secs(DISABLED_POLL_SECS)) {
                return;
            }
            continue;
        }
        let schedule = match CronSchedule::parse(&cfg.schedule) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    schedule = %cfg.schedule,
                    error = %e,
                    "dream_cycle: invalid cron schedule; sleeping {DISABLED_POLL_SECS}s",
                );
                if wait(&stop_rx, Duration::from_secs(DISABLED_POLL_SECS)) {
                    return;
                }
                continue;
            }
        };
        let now = Utc::now();
        let Some(next_fire) = schedule.next_after(now) else {
            tracing::warn!(
                schedule = %cfg.schedule,
                "dream_cycle: schedule has no next fire; sleeping {DISABLED_POLL_SECS}s",
            );
            if wait(&stop_rx, Duration::from_secs(DISABLED_POLL_SECS)) {
                return;
            }
            continue;
        };
        let secs_until = (next_fire - now).num_seconds();
        let until = u64::try_from(secs_until)
            .unwrap_or(MIN_SLEEP_SECS)
            .max(MIN_SLEEP_SECS);
        let sleep_dur = Duration::from_secs(until);
        if wait(&stop_rx, sleep_dur) {
            return;
        }
        let started = Instant::now();
        tracing::info!(
            schedule = %cfg.schedule,
            "dream_cycle: firing cycle",
        );
        match rt.block_on(run_cycle(ctx, &cfg)) {
            Ok(report) => {
                tracing::info!(
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    merged = report.merged,
                    review = report.review,
                    decayed = report.relations_decayed,
                    enriched = report.entities_enriched,
                    inferred = report.proposals_total,
                    "dream_cycle: cycle complete",
                );
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "dream_cycle: cycle failed",
                );
            }
        }
    }
}

/// Returns `true` when a stop signal was received during the wait.
fn wait(stop_rx: &mpsc::Receiver<()>, dur: Duration) -> bool {
    matches!(
        stop_rx.recv_timeout(dur),
        Ok(()) | Err(RecvTimeoutError::Disconnected)
    )
}

fn load_settings(
    forge_root: &std::path::Path,
) -> Result<nexus_formats::config::DreamCycleSettings> {
    let cfg =
        nexus_formats::config::load_app_config(forge_root).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(cfg.dream_cycle)
}

/// Per-cycle summary, surfaced via tracing.
#[derive(Debug, Default)]
struct CycleReport {
    merged: u32,
    review: u32,
    relations_decayed: u32,
    entities_enriched: u32,
    proposals_total: u32,
}

async fn run_cycle(
    ctx: &Arc<KernelPluginContext>,
    cfg: &nexus_formats::config::DreamCycleSettings,
) -> Result<CycleReport> {
    use nexus_kernel::{Events as _, Ipc as _};

    let mut report = CycleReport::default();

    // ── dedup ─────────────────────────────────────────────────────────────
    let dedup_floor = cfg.review_threshold.min(cfg.merge_threshold);
    let dup_resp = ctx
        .ipc_call(
            "com.nexus.storage",
            "entity_find_duplicates",
            serde_json::json!({ "threshold": dedup_floor }),
            IPC_TIMEOUT,
        )
        .await
        .map_err(|e| anyhow::anyhow!("entity_find_duplicates: {e}"))?;
    if let Some(pairs) = dup_resp.get("pairs").and_then(serde_json::Value::as_array) {
        let mut consumed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for pair in pairs {
            let sim = pair
                .get("similarity")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0) as f32;
            let a = pair
                .get("a")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let b = pair
                .get("b")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if a.is_empty() || b.is_empty() {
                continue;
            }
            if sim >= cfg.merge_threshold {
                if consumed.contains(a) || consumed.contains(b) {
                    continue;
                }
                match ctx
                    .ipc_call(
                        "com.nexus.storage",
                        "entity_merge",
                        serde_json::json!({ "keep": a, "drop": b }),
                        IPC_TIMEOUT,
                    )
                    .await
                {
                    Ok(_) => {
                        report.merged += 1;
                        consumed.insert(b.to_string());
                    }
                    Err(e) => {
                        tracing::warn!(keep = %a, drop = %b, error = %e, "dream_cycle: merge failed");
                    }
                }
            } else if sim >= cfg.review_threshold {
                report.review += 1;
            }
        }
    }

    // ── decay ─────────────────────────────────────────────────────────────
    let decay_resp = ctx
        .ipc_call(
            "com.nexus.storage",
            "entity_decay_relations",
            serde_json::json!({
                "factor": cfg.decay_factor,
                "floor":  cfg.decay_floor,
            }),
            IPC_TIMEOUT,
        )
        .await
        .map_err(|e| anyhow::anyhow!("entity_decay_relations: {e}"))?;
    if let Some(n) = decay_resp
        .get("relations_decayed")
        .and_then(serde_json::Value::as_u64)
    {
        report.relations_decayed = u32::try_from(n).unwrap_or(u32::MAX);
    }

    // ── enrich + infer ────────────────────────────────────────────────────
    // Iterate every entity. The handlers themselves short-circuit when
    // the description is already substantial (enrich) or when there
    // are no candidate targets / AI provider (infer).
    let entities_resp = ctx
        .ipc_call(
            "com.nexus.storage",
            "entity_search",
            serde_json::json!({ "query": "", "limit": 5000 }),
            IPC_TIMEOUT,
        )
        .await
        .map_err(|e| anyhow::anyhow!("entity_search: {e}"))?;
    if let Some(results) = entities_resp
        .get("results")
        .and_then(serde_json::Value::as_array)
    {
        for hit in results {
            let id = match hit.get("id").and_then(serde_json::Value::as_str) {
                Some(id) if !id.is_empty() => id.to_string(),
                _ => continue,
            };
            if let Ok(reply) = ctx
                .ipc_call(
                    "com.nexus.ai",
                    "enrich_entity",
                    serde_json::json!({ "entity_id": id }),
                    IPC_TIMEOUT,
                )
                .await
            {
                if reply
                    .get("applied")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
                {
                    report.entities_enriched += 1;
                }
            }
            if let Ok(reply) = ctx
                .ipc_call(
                    "com.nexus.ai",
                    "infer_entity_relations",
                    serde_json::json!({ "entity_id": id }),
                    IPC_TIMEOUT,
                )
                .await
            {
                if let Some(arr) = reply.get("proposals").and_then(serde_json::Value::as_array) {
                    report.proposals_total += u32::try_from(arr.len()).unwrap_or(0);
                }
            }
        }
    }

    if report.proposals_total > 0 {
        // Emit a kernel event so the shell can render a "N new
        // relation proposals from Dream Cycle" notification. The
        // payload includes the total + the cycle's tracing timestamp
        // so duplicate-notification dedup is straightforward.
        if let Err(e) = ctx.publish(
            "com.nexus.dream_cycle.proposals",
            serde_json::json!({
                "proposals_total":   report.proposals_total,
                "entities_enriched": report.entities_enriched,
                "merged":            report.merged,
                "review":            report.review,
            }),
        ) {
            tracing::debug!(error = %e, "dream_cycle: publish notification failed");
        }
    }

    Ok(report)
}
