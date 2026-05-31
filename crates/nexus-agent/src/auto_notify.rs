//! BL-133 follow-up — background-agent auto-notify.
//!
//! Subscribes to `com.nexus.agent.session_completed` and dispatches
//! `com.nexus.notifications::send` when the session's `duration_ms`
//! exceeds the configured threshold. The threshold lives in
//! `<forge>/.forge/config.toml` `[agent].auto_notify_threshold_s`
//! (default 30s). When the value resolves to `0` the subscriber stays
//! off — a forge that doesn't want background notifications pays no
//! IPC cost.
//!
//! The subscriber is best-effort: missing config, missing notifications
//! plugin, or unparseable payloads all log + skip rather than crashing.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use nexus_kernel::{EventFilter, Events as _, Ipc as _, KernelPluginContext, NexusEvent};
use serde::Deserialize;

/// Bus topic emitted by `handle_session_run` after a session finishes.
pub const SESSION_COMPLETED_TOPIC: &str = "com.nexus.agent.session_completed";

/// Default threshold when `[agent].auto_notify_threshold_s` is absent.
pub const DEFAULT_THRESHOLD_S: u64 = 30;

const NOTIFY_IPC_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Deserialize, Default, Clone, Debug)]
struct AgentSection {
    #[serde(default)]
    auto_notify_threshold_s: Option<u64>,
}

#[derive(Deserialize, Default)]
struct ConfigWrapper {
    #[serde(default)]
    agent: Option<AgentSection>,
}

/// Read `[agent].auto_notify_threshold_s` from `<forge>/.forge/config.toml`.
///
/// Returns [`DEFAULT_THRESHOLD_S`] when the file is missing, the section
/// is absent, or the value fails to parse. Returns `0` only when the
/// user explicitly sets it to `0` (which disables auto-notify).
#[must_use]
pub fn load_threshold_secs(forge_root: &Path) -> u64 {
    let path = forge_root.join(".forge").join("config.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return DEFAULT_THRESHOLD_S,
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                %err,
                "auto_notify: read failed; using default threshold"
            );
            return DEFAULT_THRESHOLD_S;
        }
    };
    match toml::from_str::<ConfigWrapper>(&text) {
        Ok(w) => w
            .agent
            .and_then(|a| a.auto_notify_threshold_s)
            .unwrap_or(DEFAULT_THRESHOLD_S),
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                %err,
                "auto_notify: [agent] parse failed; using default threshold"
            );
            DEFAULT_THRESHOLD_S
        }
    }
}

/// Compute milliseconds elapsed between two RFC 3339 timestamps. Returns
/// `None` when either is unparseable or `ended` is before `started`.
#[must_use]
pub fn duration_ms_between(started_at: &str, ended_at: &str) -> Option<u64> {
    let s = chrono::DateTime::parse_from_rfc3339(started_at).ok()?;
    let e = chrono::DateTime::parse_from_rfc3339(ended_at).ok()?;
    let delta = e.signed_duration_since(s).num_milliseconds();
    if delta < 0 {
        return None;
    }
    u64::try_from(delta).ok()
}

/// Format the notification message for a completed session. Pure helper
/// so the shape can be asserted in unit tests without a kernel mock.
#[must_use]
pub fn format_message(goal: &str, duration_ms: u64, outcome: &str) -> String {
    let secs = duration_ms / 1000;
    let goal_trim = goal.trim();
    let goal_display = if goal_trim.is_empty() {
        "(no goal)"
    } else {
        goal_trim
    };
    format!("Agent session finished in {secs}s ({outcome}): {goal_display}")
}

/// Spawn the subscriber on the current tokio runtime. Best-effort —
/// returns silently when no tokio handle is in scope (CLI single-shot
/// path) or when `threshold_secs == 0` (auto-notify disabled).
pub fn spawn(ctx: Arc<KernelPluginContext>, threshold_secs: u64) {
    if threshold_secs == 0 {
        tracing::debug!("auto_notify: threshold is 0, subscriber not spawned");
        return;
    }
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        tracing::debug!(
            "auto_notify: no tokio runtime in scope, subscriber not spawned (CLI path)"
        );
        return;
    };
    let mut sub = ctx.subscribe(EventFilter::CustomExact(
        SESSION_COMPLETED_TOPIC.to_string(),
    ));
    let threshold_ms = threshold_secs.saturating_mul(1000);
    handle.spawn(async move {
        tracing::debug!(
            threshold_secs,
            "auto_notify: subscriber listening on com.nexus.agent.session_completed"
        );
        loop {
            match sub.recv().await {
                Ok(evt) => {
                    let NexusEvent::Custom { payload, .. } = &evt.event else {
                        continue;
                    };
                    let Some(duration_ms) = payload
                        .get("duration_ms")
                        .and_then(serde_json::Value::as_u64)
                    else {
                        continue;
                    };
                    if duration_ms < threshold_ms {
                        continue;
                    }
                    let goal = payload
                        .get("goal")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    let outcome = payload
                        .get("outcome")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("completed");
                    let message = format_message(goal, duration_ms, outcome);
                    let body = serde_json::json!({
                        "source": "agent",
                        "title": "Agent session complete",
                        "message": message,
                    });
                    if let Err(err) = ctx
                        .ipc_call("com.nexus.notifications", "send", body, NOTIFY_IPC_TIMEOUT)
                        .await
                    {
                        // Notifications may not be loaded in every
                        // frontend; trace at debug so a stripped-down
                        // build doesn't spam warnings.
                        tracing::debug!(%err, "auto_notify: notifications::send failed");
                    }
                }
                Err(nexus_kernel::RecvError::Lagged(_)) => continue,
                Err(nexus_kernel::RecvError::Closed) => break,
            }
        }
        tracing::debug!("auto_notify: subscriber closed");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_threshold_returns_default_when_config_missing() {
        let dir = TempDir::new().expect("tempdir");
        assert_eq!(load_threshold_secs(dir.path()), DEFAULT_THRESHOLD_S);
    }

    #[test]
    fn load_threshold_reads_explicit_value() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        std::fs::write(
            dir.path().join(".forge/config.toml"),
            "[agent]\nauto_notify_threshold_s = 120\n",
        )
        .unwrap();
        assert_eq!(load_threshold_secs(dir.path()), 120);
    }

    #[test]
    fn load_threshold_zero_disables_subscriber() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        std::fs::write(
            dir.path().join(".forge/config.toml"),
            "[agent]\nauto_notify_threshold_s = 0\n",
        )
        .unwrap();
        assert_eq!(load_threshold_secs(dir.path()), 0);
    }

    #[test]
    fn load_threshold_falls_back_on_garbage() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        std::fs::write(dir.path().join(".forge/config.toml"), "not = valid = toml").unwrap();
        assert_eq!(load_threshold_secs(dir.path()), DEFAULT_THRESHOLD_S);
    }

    #[test]
    fn duration_ms_between_returns_positive_delta() {
        let s = "2026-05-17T12:00:00Z";
        let e = "2026-05-17T12:00:45Z";
        assert_eq!(duration_ms_between(s, e), Some(45_000));
    }

    #[test]
    fn duration_ms_between_returns_none_when_ended_precedes_started() {
        let s = "2026-05-17T12:00:45Z";
        let e = "2026-05-17T12:00:00Z";
        assert!(duration_ms_between(s, e).is_none());
    }

    #[test]
    fn duration_ms_between_returns_none_for_bad_timestamp() {
        assert!(duration_ms_between("not-a-date", "2026-05-17T12:00:00Z").is_none());
    }

    #[test]
    fn format_message_includes_seconds_and_goal() {
        let msg = format_message("refactor login", 45_000, "completed");
        assert!(msg.contains("45s"));
        assert!(msg.contains("refactor login"));
        assert!(msg.contains("completed"));
    }

    #[test]
    fn format_message_substitutes_empty_goal() {
        let msg = format_message("", 30_000, "completed");
        assert!(msg.contains("(no goal)"));
    }
}
