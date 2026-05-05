//! AIG-03 — trigger-spec validation.
//!
//! Wired into [`crate::parse::parse_workflow_text`] so a workflow
//! with an unparseable cron expression / non-`/` webhook path /
//! invalid `file_event` regex is rejected at *validate* time, not
//! silently logged-and-skipped at runtime when the scheduler tries
//! to arm it.
//!
//! Until BL-052 the runtime parsers (`WebhookSpec::from_trigger`,
//! `FileEventSpec::from_trigger`) catch the same errors but their
//! failures only show up in `tracing::warn!` lines — meaning a user
//! editing a workflow could save a broken config and never know
//! the workflow won't fire. Surfacing the same checks at validate
//! time means the editor / `nexus workflow validate` CLI / IPC
//! handler all reject malformed configs synchronously.
//!
//! Unknown trigger types are accepted — the trigger system is
//! deliberately extensible (community plugins can register new
//! `trigger_type` values), so we only validate the kinds the core
//! scheduler knows about.

use crate::cron::CronSchedule;
use crate::webhook::WebhookSpec;
use crate::Workflow;

/// Validate a workflow's `[trigger]` block beyond the structural
/// "type is non-empty" check. Returns `Ok(())` for unknown trigger
/// types so community-plugin trigger kinds round-trip without
/// false-positive rejections.
///
/// # Errors
/// Returns a human-readable message when the trigger config is
/// malformed for one of the kinds the core scheduler knows about
/// (`cron`, `webhook`, `file_event`).
pub fn validate_trigger(wf: &Workflow) -> Result<(), String> {
    // `manual` and unrecognised types both pass through. Listing
    // `manual` explicitly keeps the dispatch readable; the empty-
    // body fall-through covers community-plugin trigger kinds.
    match wf.trigger.trigger_type.as_str() {
        "cron" => validate_cron_trigger(wf),
        "webhook" => {
            // The webhook spec parser already does full validation
            // (path / method / secret); we just discard its output.
            // Use the workflow name as the dispatch label — irrelevant
            // for validation since we're not arming anything.
            WebhookSpec::from_trigger(&wf.workflow.name, wf).map(|_| ())
        }
        "file_event" => validate_file_event_trigger(wf),
        _ => Ok(()),
    }
}

fn validate_cron_trigger(wf: &Workflow) -> Result<(), String> {
    let schedule = wf
        .trigger
        .extra
        .get("schedule")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "cron trigger missing `schedule` string".to_string())?;
    if schedule.trim().is_empty() {
        return Err("cron trigger `schedule` cannot be empty".into());
    }
    CronSchedule::parse(schedule)
        .map(|_| ())
        .map_err(|e| format!("cron trigger has invalid schedule `{schedule}`: {e}"))
}

fn validate_file_event_trigger(wf: &Workflow) -> Result<(), String> {
    // Mirrors `FileEventSpec::from_trigger` in core_plugin.rs but
    // doesn't construct the spec — pure validation only.
    if let Some(p) = wf.trigger.extra.get("pattern").and_then(|v| v.as_str()) {
        regex_lite::Regex::new(p)
            .map(|_| ())
            .map_err(|e| format!("file_event trigger has invalid pattern regex `{p}`: {e}"))?;
    }
    if let Some(value) = wf.trigger.extra.get("events") {
        let toml::Value::Array(items) = value else {
            return Err(
                "file_event trigger `events` must be an array of strings".into(),
            );
        };
        for item in items {
            let Some(s) = item.as_str() else {
                return Err(
                    "file_event trigger `events` array must contain only strings".into(),
                );
            };
            if !matches!(s, "created" | "modified" | "deleted") {
                return Err(format!(
                    "file_event trigger has unknown event `{s}` (expected created|modified|deleted)"
                ));
            }
        }
        if items.is_empty() {
            return Err(
                "file_event trigger `events` array cannot be empty (omit the key for the default)".into(),
            );
        }
    }
    if let Some(watch_dir) = wf.trigger.extra.get("watch_dir") {
        let Some(_) = watch_dir.as_str() else {
            return Err("file_event trigger `watch_dir` must be a string".into());
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_workflow_text;

    fn parse(src: &str) -> Workflow {
        // Use the lower-level toml decode so the structural validate()
        // doesn't run — we want to test trigger validation in isolation,
        // which is reachable through parse_workflow_text but the
        // failure mode there is the new error variant.
        toml::from_str(src).expect("toml decode")
    }

    // ── cron ────────────────────────────────────────────────────────

    #[test]
    fn cron_accepts_valid_schedule() {
        let wf = parse(
            r#"
[workflow]
name = "Daily"
[trigger]
type = "cron"
schedule = "0 9 * * *"
"#,
        );
        validate_trigger(&wf).expect("valid cron should pass");
    }

    #[test]
    fn cron_rejects_missing_schedule() {
        let wf = parse(
            r#"
[workflow]
name = "Daily"
[trigger]
type = "cron"
"#,
        );
        let err = validate_trigger(&wf).expect_err("missing schedule should fail");
        assert!(err.contains("missing `schedule`"), "got: {err}");
    }

    #[test]
    fn cron_rejects_unparseable_schedule() {
        let wf = parse(
            r#"
[workflow]
name = "Daily"
[trigger]
type = "cron"
schedule = "not a cron expr"
"#,
        );
        let err = validate_trigger(&wf).expect_err("garbage schedule should fail");
        assert!(err.contains("invalid schedule"), "got: {err}");
    }

    // ── webhook ─────────────────────────────────────────────────────

    #[test]
    fn webhook_accepts_canonical_shape() {
        let wf = parse(
            r#"
[workflow]
name = "WH"
[trigger]
type = "webhook"
path = "/gitlab-push"
secret = "shh"
"#,
        );
        validate_trigger(&wf).expect("valid webhook should pass");
    }

    #[test]
    fn webhook_rejects_path_without_leading_slash() {
        let wf = parse(
            r#"
[workflow]
name = "WH"
[trigger]
type = "webhook"
path = "gitlab-push"
"#,
        );
        let err = validate_trigger(&wf).expect_err("non-/ path should fail");
        assert!(err.contains("must start with '/'"), "got: {err}");
    }

    #[test]
    fn webhook_rejects_unsupported_method() {
        let wf = parse(
            r#"
[workflow]
name = "WH"
[trigger]
type = "webhook"
path = "/x"
method = "GET"
"#,
        );
        let err = validate_trigger(&wf).expect_err("GET method should fail");
        assert!(err.contains("must be POST"), "got: {err}");
    }

    // ── file_event ──────────────────────────────────────────────────

    #[test]
    fn file_event_accepts_canonical_shape() {
        let wf = parse(
            r#"
[workflow]
name = "FE"
[trigger]
type = "file_event"
watch_dir = "notes/"
pattern = "\\.md$"
events = ["created", "modified"]
"#,
        );
        validate_trigger(&wf).expect("valid file_event should pass");
    }

    #[test]
    fn file_event_rejects_invalid_regex() {
        let wf = parse(
            r#"
[workflow]
name = "FE"
[trigger]
type = "file_event"
pattern = "[unclosed"
"#,
        );
        let err = validate_trigger(&wf).expect_err("bad regex should fail");
        assert!(err.contains("invalid pattern regex"), "got: {err}");
    }

    #[test]
    fn file_event_rejects_unknown_event_name() {
        let wf = parse(
            r#"
[workflow]
name = "FE"
[trigger]
type = "file_event"
events = ["created", "renamed"]
"#,
        );
        let err = validate_trigger(&wf).expect_err("unknown event should fail");
        assert!(err.contains("unknown event"), "got: {err}");
    }

    #[test]
    fn file_event_rejects_empty_events_array() {
        let wf = parse(
            r#"
[workflow]
name = "FE"
[trigger]
type = "file_event"
events = []
"#,
        );
        let err = validate_trigger(&wf).expect_err("empty events should fail");
        assert!(err.contains("cannot be empty"), "got: {err}");
    }

    #[test]
    fn file_event_rejects_non_array_events() {
        let wf = parse(
            r#"
[workflow]
name = "FE"
[trigger]
type = "file_event"
events = "created"
"#,
        );
        let err = validate_trigger(&wf).expect_err("non-array events should fail");
        assert!(err.contains("must be an array"), "got: {err}");
    }

    // ── unknown / manual ────────────────────────────────────────────

    #[test]
    fn unknown_trigger_type_is_accepted() {
        let wf = parse(
            r#"
[workflow]
name = "X"
[trigger]
type = "community.custom_trigger"
some_field = 42
"#,
        );
        // Don't reject — community plugins register their own
        // trigger kinds and we don't know how to validate them.
        validate_trigger(&wf).expect("unknown trigger type should pass");
    }

    #[test]
    fn manual_trigger_is_accepted() {
        let wf = parse(
            r#"
[workflow]
name = "M"
[trigger]
type = "manual"
"#,
        );
        validate_trigger(&wf).expect("manual trigger should pass");
    }

    // ── parse_workflow_text wiring ──────────────────────────────────

    #[test]
    fn parse_workflow_text_rejects_invalid_trigger() {
        let src = r#"
[workflow]
name = "BadCron"
[trigger]
type = "cron"
schedule = "not a cron"
"#;
        let err = parse_workflow_text(src)
            .expect_err("invalid cron should be rejected at parse time");
        let msg = err.to_string();
        assert!(msg.contains("invalid schedule"), "got: {msg}");
    }
}
