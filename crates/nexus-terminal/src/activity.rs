//! BL-057 — translation from [`TerminalEvent`] to the universal activity log.
//!
//! The kernel publishes activity entries on `com.nexus.activity.appended`
//! so the BL-052 activity-timeline pane sees terminal session boundaries
//! alongside AI / file / git activity without re-implementing event parsing.

use crate::server::TerminalEvent;

/// Translate a [`TerminalEvent`] session-boundary into an
/// [`nexus_types::activity::ActivityEntry`] tagged with the
/// `terminal:<session_id>` origin and `process` surface. Returns
/// `None` for streaming variants we don't want to surface.
pub(crate) fn build_activity_entry(
    event: &TerminalEvent,
) -> Option<nexus_types::activity::ActivityEntry> {
    use nexus_types::activity::{ActivityEntry, ActivityOrigin, ActivityOutcome, ActivitySurface};

    let session_id = event.session_id().to_string();
    let mut entry = ActivityEntry::now(
        session_id.clone(),
        ActivitySurface::Process,
        ActivityOrigin::Terminal(session_id.clone()),
    );

    match event {
        TerminalEvent::SessionCreated { id, name } => {
            entry.outcome = ActivityOutcome::Ok;
            entry.prompt = match name {
                Some(n) => format!("started session {n}"),
                None => format!("started session {id}"),
            };
        }
        TerminalEvent::SessionClosed { id, exit_code } => {
            // Treat exit code 0 / unknown as Ok; non-zero as Error so
            // the timeline can flash an error glyph for failed runs.
            match exit_code {
                Some(0) | None => {
                    entry.outcome = ActivityOutcome::Ok;
                    entry.prompt = format!(
                        "session {id} exited (code={})",
                        exit_code.map_or("?".to_string(), |c| c.to_string()),
                    );
                }
                Some(code) => {
                    entry.outcome = ActivityOutcome::Error;
                    entry.prompt = format!("session {id} exited (code={code})");
                    entry.error = Some(format!("non-zero exit code {code}"));
                }
            }
        }
        TerminalEvent::MemoryLimitExceeded {
            id,
            rss_bytes,
            limit_mb,
        } => {
            entry.outcome = ActivityOutcome::Error;
            entry.prompt = format!("session {id} killed (OOM): rss={rss_bytes} limit={limit_mb}MB");
            entry.error = Some(format!("memory limit exceeded ({limit_mb}MB)"));
        }
        // #409 — warning-only: the process keeps running, so this is
        // `Ok` rather than `Error` (contrast the hard-kill arm above).
        TerminalEvent::SoftLimitExceeded {
            id,
            rss_bytes,
            limit_mb,
        } => {
            entry.outcome = ActivityOutcome::Ok;
            entry.prompt =
                format!("session {id} approaching memory limit: rss={rss_bytes} limit={limit_mb}MB");
        }
        // Streaming / internal variants don't reach the activity log.
        // A rename is a UI-label tweak, not a session-boundary event, so
        // it's intentionally excluded here too — it still flows on the
        // per-session lifecycle topic via `publish_lifecycle_event`.
        // CommandFinished (OSC 133) is a per-command agent signal carried on the
        // lifecycle topic, not a session boundary, and it lacks the command line
        // needed for a useful timeline row — so it's excluded here too.
        TerminalEvent::OutputReceived { .. }
        | TerminalEvent::PatternMatched { .. }
        | TerminalEvent::SessionRenamed { .. }
        | TerminalEvent::SessionEvicted { .. }
        | TerminalEvent::CommandFinished { .. } => return None,
    }
    Some(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_types::activity::ActivityOutcome;

    #[test]
    fn soft_limit_exceeded_is_ok_outcome_with_no_error_field() {
        let entry = build_activity_entry(&TerminalEvent::SoftLimitExceeded {
            id: "s1".to_string(),
            rss_bytes: 260_000_000,
            limit_mb: 250,
        })
        .expect("SoftLimitExceeded must produce an activity entry");
        assert_eq!(entry.outcome, ActivityOutcome::Ok);
        assert!(entry.error.is_none());
        assert!(entry.prompt.contains("s1"));
        assert!(entry.prompt.contains("260000000"));
        assert!(entry.prompt.contains("250"));
    }

    #[test]
    fn hard_limit_exceeded_is_error_outcome_with_error_field() {
        // #409 — regression: this arm previously never ran in
        // production because the poller published MemoryLimitExceeded
        // directly on the bus instead of through
        // `publish_lifecycle_event` (the only caller of this
        // function). Locking the arm's own behavior in here so a
        // future refactor can't silently drop it again.
        let entry = build_activity_entry(&TerminalEvent::MemoryLimitExceeded {
            id: "s1".to_string(),
            rss_bytes: 600_000_000,
            limit_mb: 500,
        })
        .expect("MemoryLimitExceeded must produce an activity entry");
        assert_eq!(entry.outcome, ActivityOutcome::Error);
        assert!(entry.error.is_some());
        assert!(entry.prompt.contains("killed (OOM)"));
    }
}
