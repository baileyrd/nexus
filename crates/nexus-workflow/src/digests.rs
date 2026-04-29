//! BL-047 — scheduled digests.
//!
//! A workflow-driven cron job that walks recent capture-note markdown
//! files in the forge, asks the AI provider to summarise them, and
//! writes the result to a `Digests/` file. Two windows are supported:
//!
//! - [`DigestKind::Daily`] — last 24 hours, output
//!   `Digests/Daily-YYYY-MM-DD.md`.
//! - [`DigestKind::Weekly`] — last 7 days, output
//!   `Digests/Weekly-YYYY-Www.md` (ISO week numbering).
//!
//! This is the **one-off cron path** described in the BL-047 PRD —
//! BL-028 (template engine) is the follow-up upgrade that will let
//! users customise the prompt + frontmatter shape.
//!
//! All filesystem and AI access flows through kernel IPC
//! (`com.nexus.storage::list_dir|read_file|write_file|create_dir` and
//! `com.nexus.ai::ask`) — the workflow crate never touches the
//! filesystem or AI provider directly.

// `Option<String>` defaults are part of the user-facing config wire
// shape (`None` = "disabled"); we deliberately keep them wrapped.
#![allow(clippy::unnecessary_wraps)]
// `from_str` mirrors the JSON IPC arg name; not a `FromStr` impl.
#![allow(clippy::should_implement_trait)]
// Hot-path string building uses `format!` — measured cost is negligible
// against the IPC round-trips that follow.
#![allow(clippy::format_push_string)]
// `read_file` returns `Vec<u8>` from JSON numbers `0..=255` by
// construction; truncation cannot occur in practice.
#![allow(clippy::cast_possible_truncation)]

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Datelike, TimeZone, Utc};
use nexus_kernel::{KernelPluginContext, PluginContext};
use nexus_plugins::PluginError;
use serde::{Deserialize, Serialize};

/// Default daily-digest cron — 09:00 UTC every day.
pub const DEFAULT_DAILY_CRON: &str = "0 9 * * *";
/// Default weekly-digest cron — 09:00 UTC every Monday.
pub const DEFAULT_WEEKLY_CRON: &str = "0 9 * * 1";
/// Default subdirectory for digest output.
pub const DEFAULT_DIGESTS_DIR: &str = "Digests";

/// Per-IPC timeout when calling storage / AI from the digest pipeline.
const IPC_TIMEOUT: Duration = Duration::from_secs(120);

/// Configuration loaded from `[digests]` in `<forge>/.forge/config.toml`.
///
/// All fields default to "off but pre-configured" — users opt in by
/// flipping `enabled = true`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DigestConfig {
    /// Master switch. When `false` the cron loop never fires; manual
    /// `run_digest` IPC calls still work.
    #[serde(default)]
    pub enabled: bool,
    /// 5-field cron expression for the daily digest. `None` disables.
    #[serde(default = "default_daily")]
    pub daily_cron: Option<String>,
    /// 5-field cron expression for the weekly digest. `None` disables.
    #[serde(default = "default_weekly")]
    pub weekly_cron: Option<String>,
    /// Forge-relative subtree to scan. `None` means the whole forge.
    #[serde(default)]
    pub scope_path: Option<String>,
    /// Forge-relative directory where digest files are written.
    #[serde(default = "default_digests_dir")]
    pub digests_dir: String,
}

fn default_daily() -> Option<String> {
    Some(DEFAULT_DAILY_CRON.to_string())
}
fn default_weekly() -> Option<String> {
    Some(DEFAULT_WEEKLY_CRON.to_string())
}
fn default_digests_dir() -> String {
    DEFAULT_DIGESTS_DIR.to_string()
}

impl Default for DigestConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            daily_cron: default_daily(),
            weekly_cron: default_weekly(),
            scope_path: None,
            digests_dir: default_digests_dir(),
        }
    }
}

/// Which window to summarise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DigestKind {
    /// Last 24 hours.
    Daily,
    /// Last 7 days.
    Weekly,
}

impl DigestKind {
    /// Parse from the IPC arg string.
    ///
    /// # Errors
    /// Returns `Err` for any value other than `"daily"` / `"weekly"`.
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "daily" => Ok(Self::Daily),
            "weekly" => Ok(Self::Weekly),
            other => Err(format!(
                "unknown digest kind '{other}' (expected daily|weekly)"
            )),
        }
    }
}

/// Result returned by [`run_digest`] and the IPC handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DigestRunReport {
    /// Which window was summarised.
    pub kind: DigestKind,
    /// Number of source files included.
    pub source_count: usize,
    /// Forge-relative path of the written digest file.
    pub output_path: String,
    /// Model identifier reported by the AI provider, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Whether the digest was actually written. `false` when no source
    /// files fell into the window — the pipeline short-circuits to
    /// avoid asking the model to summarise nothing.
    pub written: bool,
}

/// Compute `(start, end)` of the digest window, exclusive on `end`.
///
/// - Daily → `[now - 24h, now]`.
/// - Weekly → `[now - 7d, now]`.
#[must_use]
pub fn digest_window(kind: DigestKind, now_utc: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let span = match kind {
        DigestKind::Daily => chrono::Duration::days(1),
        DigestKind::Weekly => chrono::Duration::days(7),
    };
    (now_utc - span, now_utc)
}

/// Build the forge-relative output path for a given run.
///
/// - Daily → `<dir>/Daily-YYYY-MM-DD.md`.
/// - Weekly → `<dir>/Weekly-YYYY-Www.md` (ISO 8601 week numbering;
///   `YYYY` is the **ISO** year, which can differ from the calendar
///   year at year boundaries).
#[must_use]
pub fn output_path(kind: DigestKind, now_utc: DateTime<Utc>, digests_dir: &str) -> String {
    let dir = digests_dir.trim_end_matches('/');
    match kind {
        DigestKind::Daily => format!("{dir}/Daily-{}.md", now_utc.format("%Y-%m-%d")),
        DigestKind::Weekly => {
            let iso = now_utc.iso_week();
            format!("{dir}/Weekly-{}-W{:02}.md", iso.year(), iso.week())
        }
    }
}

/// Build the AI prompt for a digest run.
///
/// Each source is included as a fenced markdown block headed by its
/// path. The leading instruction tells the model to emit a concise
/// markdown digest — no template engine yet (BL-028 will plug one in).
#[must_use]
pub fn build_digest_prompt(kind: DigestKind, sources: &[(String, String)]) -> String {
    let label = match kind {
        DigestKind::Daily => "the last 24 hours",
        DigestKind::Weekly => "the last 7 days",
    };
    let mut out = String::new();
    out.push_str(&format!(
        "You are summarising capture notes from {label}. Produce a concise \
         markdown digest with: a short overview paragraph, a bulleted \
         list of key themes, and a bulleted list of action items if any \
         appear. Cite source files inline as `path/to/file.md`.\n\n",
    ));
    out.push_str(&format!("# Sources ({} file(s))\n\n", sources.len()));
    for (path, body) in sources {
        out.push_str(&format!("## {path}\n\n"));
        out.push_str("```markdown\n");
        out.push_str(body);
        if !body.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("```\n\n");
    }
    out
}

/// Run a digest end-to-end.
///
/// Walks the configured scope, filters markdown files whose mtime
/// falls inside the digest window, asks `com.nexus.ai::ask` for the
/// summary, ensures `digests_dir` exists, then writes the answer.
///
/// # Errors
/// Surfaces any IPC failure from the storage or AI plugins.
pub async fn run_digest(
    ctx: &Arc<KernelPluginContext>,
    config: &DigestConfig,
    kind: DigestKind,
    now_utc: DateTime<Utc>,
) -> Result<DigestRunReport, PluginError> {
    let (start, end) = digest_window(kind, now_utc);
    let scope = config.scope_path.as_deref().unwrap_or("");
    let mut files = Vec::new();
    walk_markdown(ctx, scope, &mut files).await?;

    let in_window: Vec<String> = files
        .into_iter()
        .filter_map(|(relpath, mtime_ms)| {
            let mtime_ms = mtime_ms?;
            let ts = Utc.timestamp_millis_opt(mtime_ms).single()?;
            if ts >= start && ts < end {
                Some(relpath)
            } else {
                None
            }
        })
        .collect();

    let out_relpath = output_path(kind, now_utc, &config.digests_dir);

    if in_window.is_empty() {
        return Ok(DigestRunReport {
            kind,
            source_count: 0,
            output_path: out_relpath,
            model: None,
            written: false,
        });
    }

    let mut sources: Vec<(String, String)> = Vec::with_capacity(in_window.len());
    for relpath in in_window {
        // Skip the digest output area to avoid feedback loops.
        if relpath.starts_with(&format!("{}/", config.digests_dir)) {
            continue;
        }
        let body = read_file(ctx, &relpath).await?;
        sources.push((relpath, body));
    }

    if sources.is_empty() {
        return Ok(DigestRunReport {
            kind,
            source_count: 0,
            output_path: out_relpath,
            model: None,
            written: false,
        });
    }

    let prompt = build_digest_prompt(kind, &sources);
    let (answer, model) = ask_ai(ctx, &prompt).await?;

    create_dir(ctx, &config.digests_dir).await?;
    let body = format_digest_body(kind, now_utc, &answer, &sources);
    write_file(ctx, &out_relpath, body.as_bytes()).await?;

    Ok(DigestRunReport {
        kind,
        source_count: sources.len(),
        output_path: out_relpath,
        model,
        written: true,
    })
}

/// Render the digest file body, prepending YAML frontmatter so it can
/// be re-indexed and back-referenced by the rest of the system.
fn format_digest_body(
    kind: DigestKind,
    now_utc: DateTime<Utc>,
    answer: &str,
    sources: &[(String, String)],
) -> String {
    let kind_str = match kind {
        DigestKind::Daily => "daily",
        DigestKind::Weekly => "weekly",
    };
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("digest_kind: {kind_str}\n"));
    out.push_str(&format!(
        "generated_at: {}\n",
        now_utc.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    ));
    out.push_str(&format!("source_count: {}\n", sources.len()));
    out.push_str("sources:\n");
    for (p, _) in sources {
        out.push_str(&format!("  - {p}\n"));
    }
    out.push_str("---\n\n");
    out.push_str(answer);
    if !answer.ends_with('\n') {
        out.push('\n');
    }
    out
}

// ── IPC helpers ─────────────────────────────────────────────────────────────

/// Recursively list markdown files under `relpath`, returning
/// `(relpath, modified_ms)` for each `.md` regular file.
async fn walk_markdown(
    ctx: &Arc<KernelPluginContext>,
    relpath: &str,
    out: &mut Vec<(String, Option<i64>)>,
) -> Result<(), PluginError> {
    let value = ctx
        .ipc_call(
            "com.nexus.storage",
            "list_dir",
            serde_json::json!({ "relpath": relpath }),
            IPC_TIMEOUT,
        )
        .await
        .map_err(|e| ipc_err(format!("list_dir({relpath}): {e}")))?;
    let entries = value
        .as_array()
        .ok_or_else(|| ipc_err("list_dir: expected array".into()))?;
    for entry in entries {
        let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
        // Skip hidden + the .forge index dir.
        if name.starts_with('.') {
            continue;
        }
        let entry_relpath = entry
            .get("relpath")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let is_dir = entry.get("isDir").and_then(serde_json::Value::as_bool)
            .or_else(|| entry.get("is_dir").and_then(serde_json::Value::as_bool))
            .unwrap_or(false);
        if is_dir {
            // Recurse — boxed to avoid an infinite-size async future.
            Box::pin(walk_markdown(ctx, &entry_relpath, out)).await?;
            continue;
        }
        if !entry_relpath.to_lowercase().ends_with(".md") {
            continue;
        }
        let mtime = entry
            .get("modifiedMs")
            .and_then(serde_json::Value::as_i64)
            .or_else(|| entry.get("modified_ms").and_then(serde_json::Value::as_i64));
        out.push((entry_relpath, mtime));
    }
    Ok(())
}

async fn read_file(ctx: &Arc<KernelPluginContext>, relpath: &str) -> Result<String, PluginError> {
let v = ctx
        .ipc_call(
            "com.nexus.storage",
            "read_file",
            serde_json::json!({ "path": relpath }),
            IPC_TIMEOUT,
        )
        .await
        .map_err(|e| ipc_err(format!("read_file({relpath}): {e}")))?;
    if let Some(s) = v.as_str() {
        return Ok(s.to_string());
    }
    if let Some(bytes) = v.get("bytes").and_then(serde_json::Value::as_array) {
        let raw: Vec<u8> = bytes
            .iter()
            .filter_map(serde_json::Value::as_u64)
            .map(|n| n as u8)
            .collect();
        return String::from_utf8(raw)
            .map_err(|e| ipc_err(format!("read_file({relpath}): utf8: {e}")));
    }
    if let Some(s) = v.get("text").and_then(|v| v.as_str()) {
        return Ok(s.to_string());
    }
    Err(ipc_err(format!(
        "read_file({relpath}): unrecognised response shape"
    )))
}

async fn write_file(
    ctx: &Arc<KernelPluginContext>,
    relpath: &str,
    bytes: &[u8],
) -> Result<(), PluginError> {
let bytes_array: Vec<serde_json::Value> = bytes
        .iter()
        .map(|b| serde_json::Value::from(u64::from(*b)))
        .collect();
    ctx.ipc_call(
        "com.nexus.storage",
        "write_file",
        serde_json::json!({ "path": relpath, "bytes": bytes_array }),
        IPC_TIMEOUT,
    )
    .await
    .map_err(|e| ipc_err(format!("write_file({relpath}): {e}")))?;
    Ok(())
}

async fn create_dir(ctx: &Arc<KernelPluginContext>, relpath: &str) -> Result<(), PluginError> {
match ctx
        .ipc_call(
            "com.nexus.storage",
            "create_dir",
            serde_json::json!({ "relpath": relpath }),
            IPC_TIMEOUT,
        )
        .await
    {
        Ok(_) => Ok(()),
        // `create_dir` is best-effort — a pre-existing directory should
        // not abort the digest run.
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("exists") || msg.contains("AlreadyExists") {
                Ok(())
            } else {
                Err(ipc_err(format!("create_dir({relpath}): {e}")))
            }
        }
    }
}

async fn ask_ai(
    ctx: &Arc<KernelPluginContext>,
    prompt: &str,
) -> Result<(String, Option<String>), PluginError> {
let v = ctx
        .ipc_call(
            "com.nexus.ai",
            "ask",
            serde_json::json!({ "question": prompt, "limit": 1 }),
            Duration::from_secs(180),
        )
        .await
        .map_err(|e| ipc_err(format!("ai.ask: {e}")))?;
    let answer = v
        .get("answer")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ipc_err("ai.ask: missing 'answer' field".into()))?
        .to_string();
    let model = v
        .get("model")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);
    Ok((answer, model))
}

fn ipc_err(reason: String) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: "com.nexus.workflow".to_string(),
        reason,
    }
}

/// Compute the wall-clock `Duration` until the next fire across the
/// configured daily / weekly schedules. Returns `None` if both
/// schedules are disabled or fail to parse, or if neither has a future
/// fire time within reasonable bounds.
#[must_use]
pub fn next_fire(
    config: &DigestConfig,
    now_utc: DateTime<Utc>,
) -> Option<(DigestKind, DateTime<Utc>)> {
    use crate::cron::CronSchedule;
    let mut best: Option<(DigestKind, DateTime<Utc>)> = None;
    for (kind, expr) in [
        (DigestKind::Daily, config.daily_cron.as_deref()),
        (DigestKind::Weekly, config.weekly_cron.as_deref()),
    ] {
        let Some(expr) = expr else { continue };
        let Ok(sched) = CronSchedule::parse(expr) else {
            continue;
        };
        let Some(next) = sched.next_after(now_utc) else {
            continue;
        };
        match &best {
            Some((_, t)) if *t <= next => {}
            _ => best = Some((kind, next)),
        }
    }
    best
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use nexus_kernel::{Capability, CapabilitySet, EventBus, InMemoryKvStore, IpcDispatcher, KvStore};
    use std::sync::Mutex;

    fn ts(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
    }

    #[test]
    fn defaults_are_disabled_with_known_crons() {
        let cfg = DigestConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.daily_cron.as_deref(), Some(DEFAULT_DAILY_CRON));
        assert_eq!(cfg.weekly_cron.as_deref(), Some(DEFAULT_WEEKLY_CRON));
        assert_eq!(cfg.digests_dir, DEFAULT_DIGESTS_DIR);
        assert!(cfg.scope_path.is_none());
    }

    #[test]
    fn digest_window_daily_is_last_24h() {
        let now = ts(2026, 4, 29, 12, 0);
        let (start, end) = digest_window(DigestKind::Daily, now);
        assert_eq!(end, now);
        assert_eq!(start, ts(2026, 4, 28, 12, 0));
    }

    #[test]
    fn digest_window_weekly_is_last_7d() {
        let now = ts(2026, 4, 29, 12, 0);
        let (start, end) = digest_window(DigestKind::Weekly, now);
        assert_eq!(end, now);
        assert_eq!(start, ts(2026, 4, 22, 12, 0));
    }

    #[test]
    fn output_path_daily_uses_iso_date() {
        let p = output_path(DigestKind::Daily, ts(2026, 4, 29, 9, 0), "Digests");
        assert_eq!(p, "Digests/Daily-2026-04-29.md");
    }

    #[test]
    fn output_path_weekly_uses_iso_week() {
        // 2026-04-29 (Wed) falls in ISO week 18.
        let p = output_path(DigestKind::Weekly, ts(2026, 4, 29, 9, 0), "Digests");
        assert_eq!(p, "Digests/Weekly-2026-W18.md");
    }

    #[test]
    fn output_path_weekly_iso_year_boundary() {
        // 2027-01-01 is Friday → ISO week 53 of 2026.
        let p = output_path(DigestKind::Weekly, ts(2027, 1, 1, 9, 0), "Digests");
        assert_eq!(p, "Digests/Weekly-2026-W53.md");
    }

    #[test]
    fn output_path_strips_trailing_slash() {
        let p = output_path(DigestKind::Daily, ts(2026, 4, 29, 9, 0), "Digests/");
        assert_eq!(p, "Digests/Daily-2026-04-29.md");
    }

    #[test]
    fn build_prompt_includes_sources_and_paths() {
        let sources = vec![
            ("notes/a.md".to_string(), "alpha body".to_string()),
            ("notes/b.md".to_string(), "beta body\n".to_string()),
        ];
        let prompt = build_digest_prompt(DigestKind::Daily, &sources);
        assert!(prompt.contains("last 24 hours"));
        assert!(prompt.contains("notes/a.md"));
        assert!(prompt.contains("alpha body"));
        assert!(prompt.contains("notes/b.md"));
        assert!(prompt.contains("beta body"));
        assert!(prompt.contains("# Sources (2 file(s))"));
    }

    #[test]
    fn build_prompt_weekly_label() {
        let sources = vec![("a.md".to_string(), "x".to_string())];
        let p = build_digest_prompt(DigestKind::Weekly, &sources);
        assert!(p.contains("last 7 days"));
    }

    #[test]
    fn next_fire_picks_earliest_across_schedules() {
        let cfg = DigestConfig::default();
        // Sunday 2026-04-26 08:00 UTC → daily fires today 09:00,
        // weekly fires Monday 2026-04-27 09:00. Daily wins.
        let now = ts(2026, 4, 26, 8, 0);
        let (kind, next) = next_fire(&cfg, now).expect("some schedule");
        assert_eq!(kind, DigestKind::Daily);
        assert_eq!(next, ts(2026, 4, 26, 9, 0));
    }

    /// Stub IPC dispatcher for the integration test below. Records
    /// every call and returns canned responses for the storage / AI
    /// commands `run_digest` makes. Mirrors the BL-041 stub pattern.
    struct StubDispatcher {
        files: Vec<(String, String, i64)>, // (relpath, body, mtime_ms)
        calls: Mutex<Vec<(String, String)>>,
        ai_answer: String,
    }

    impl StubDispatcher {
        fn new(files: Vec<(String, String, i64)>, ai_answer: &str) -> Arc<Self> {
            Arc::new(Self {
                files,
                calls: Mutex::new(Vec::new()),
                ai_answer: ai_answer.to_string(),
            })
        }

        fn record(&self, target: &str, command: &str) {
            self.calls
                .lock()
                .unwrap()
                .push((target.to_string(), command.to_string()));
        }
    }

    impl IpcDispatcher for StubDispatcher {
        fn dispatch(
            &self,
            target: &str,
            command: &str,
            args: &serde_json::Value,
        ) -> Result<serde_json::Value, nexus_kernel::IpcError> {
            self.record(target, command);
            match (target, command) {
                ("com.nexus.storage", "list_dir") => {
                    let relpath = args
                        .get("relpath")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    // Return only files at the requested level — no
                    // nesting in the test fixture.
                    let entries: Vec<serde_json::Value> = self
                        .files
                        .iter()
                        .filter(|(p, _, _)| {
                            if relpath.is_empty() {
                                !p.contains('/')
                            } else {
                                p.starts_with(&format!("{relpath}/"))
                                    && !p[relpath.len() + 1..].contains('/')
                            }
                        })
                        .map(|(p, _, m)| {
                            serde_json::json!({
                                "name": p.rsplit('/').next().unwrap_or(p),
                                "relpath": p,
                                "isDir": false,
                                "modifiedMs": m,
                            })
                        })
                        .collect();
                    Ok(serde_json::Value::Array(entries))
                }
                ("com.nexus.storage", "read_file") => {
                    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                    let body = self
                        .files
                        .iter()
                        .find(|(p, _, _)| p == path)
                        .map(|(_, b, _)| b.clone())
                        .unwrap_or_default();
                    Ok(serde_json::Value::String(body))
                }
                ("com.nexus.storage", "create_dir" | "write_file") => {
                    Ok(serde_json::json!({}))
                }
                ("com.nexus.ai", "ask") => Ok(serde_json::json!({
                    "answer": self.ai_answer,
                    "model": "stub-model",
                    "sources": [],
                    "citations": [],
                })),
                _ => Err(nexus_kernel::IpcError::CommandNotFound {
                    plugin_id: target.to_string(),
                    command: command.to_string(),
                }),
            }
        }
    }

    fn make_ctx(dispatcher: Arc<StubDispatcher>) -> Arc<KernelPluginContext> {
        let dir = tempfile::tempdir().unwrap();
        // Leak the tempdir so the path stays valid for the test
        // duration; the OS reclaims on process exit.
        let path = dir.keep();
        let kv: Arc<dyn KvStore> = Arc::new(InMemoryKvStore::new());
        let bus = Arc::new(EventBus::new(16));
        let caps: CapabilitySet = Capability::ALL.iter().copied().collect();
        let ctx = KernelPluginContext::new(
            "com.nexus.workflow",
            "0.0.1",
            caps,
            kv,
            bus,
            &path,
            Some(dispatcher as Arc<dyn IpcDispatcher>),
        )
        .unwrap();
        Arc::new(ctx)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_digest_walks_files_calls_ai_and_writes() {
        // Window = last 24h centred on `now`.
        let now = ts(2026, 4, 29, 12, 0);
        let in_window_ms = now.timestamp_millis() - 3_600_000; // 1h ago
        let stale_ms = now.timestamp_millis() - 5 * 86_400_000; // 5d ago

        let files = vec![
            (
                "notes/today.md".to_string(),
                "today body".to_string(),
                in_window_ms,
            ),
            (
                "notes/old.md".to_string(),
                "old body".to_string(),
                stale_ms,
            ),
            // Skipped — outside .md filter.
            (
                "notes/photo.png".to_string(),
                "binary".to_string(),
                in_window_ms,
            ),
        ];
        let dispatcher = StubDispatcher::new(files, "## Digest\n\nKey theme: testing.");
        let ctx = make_ctx(Arc::clone(&dispatcher));

        let cfg = DigestConfig {
            enabled: true,
            scope_path: Some("notes".to_string()),
            ..DigestConfig::default()
        };
        let report = run_digest(&ctx, &cfg, DigestKind::Daily, now)
            .await
            .expect("digest run");

        assert_eq!(report.kind, DigestKind::Daily);
        assert_eq!(report.source_count, 1, "only today.md is in window + .md");
        assert!(report.written);
        assert_eq!(report.output_path, "Digests/Daily-2026-04-29.md");
        assert_eq!(report.model.as_deref(), Some("stub-model"));

        let calls = dispatcher.calls.lock().unwrap();
        assert!(calls
            .iter()
            .any(|(t, c)| t == "com.nexus.storage" && c == "list_dir"));
        assert!(calls
            .iter()
            .any(|(t, c)| t == "com.nexus.storage" && c == "read_file"));
        assert!(calls
            .iter()
            .any(|(t, c)| t == "com.nexus.ai" && c == "ask"));
        assert!(calls
            .iter()
            .any(|(t, c)| t == "com.nexus.storage" && c == "write_file"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_digest_short_circuits_when_no_files_in_window() {
        let now = ts(2026, 4, 29, 12, 0);
        let stale_ms = now.timestamp_millis() - 30 * 86_400_000;
        let files = vec![(
            "notes/old.md".to_string(),
            "old".to_string(),
            stale_ms,
        )];
        let dispatcher = StubDispatcher::new(files, "unused");
        let ctx = make_ctx(Arc::clone(&dispatcher));
        let cfg = DigestConfig {
            enabled: true,
            scope_path: Some("notes".to_string()),
            ..DigestConfig::default()
        };
        let report = run_digest(&ctx, &cfg, DigestKind::Weekly, now)
            .await
            .expect("digest run");
        assert_eq!(report.source_count, 0);
        assert!(!report.written);
        let calls = dispatcher.calls.lock().unwrap();
        assert!(
            !calls.iter().any(|(_, c)| c == "ask"),
            "AI should not be called when no sources in window"
        );
    }

    #[test]
    fn digest_kind_from_str() {
        assert_eq!(DigestKind::from_str("daily").unwrap(), DigestKind::Daily);
        assert_eq!(DigestKind::from_str("weekly").unwrap(), DigestKind::Weekly);
        assert!(DigestKind::from_str("monthly").is_err());
    }

}
