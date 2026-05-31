//! `.workflow.toml` parser. Thin wrapper over `toml` with precise
//! error reporting.

use std::path::Path;

use thiserror::Error;

use crate::Workflow;

/// Errors from parsing `.workflow.toml` files.
#[derive(Debug, Error)]
pub enum WorkflowParseError {
    /// Disk read failed.
    #[error("io error reading workflow: {0}")]
    Io(#[from] std::io::Error),
    /// TOML decode failed.
    #[error("invalid workflow TOML: {0}")]
    Toml(String),
    /// Required fields missing after decode.
    #[error("workflow missing required field: {0}")]
    MissingField(&'static str),
    /// AIG-03 — trigger config rejected by the type-specific validator
    /// (invalid cron / non-`/` webhook path / malformed `file_event` regex,
    /// etc.). Carries the human-readable reason from
    /// [`crate::validate_trigger`].
    #[error("invalid trigger config: {0}")]
    InvalidTrigger(String),
}

/// Parse a workflow from an in-memory string.
///
/// # Errors
/// Returns [`WorkflowParseError::Toml`] on decode failure, or
/// [`WorkflowParseError::MissingField`] when semantic invariants
/// (non-empty name, non-empty trigger type) fail.
pub fn parse_workflow_text(s: &str) -> Result<Workflow, WorkflowParseError> {
    let workflow: Workflow =
        toml::from_str(s).map_err(|e| WorkflowParseError::Toml(e.to_string()))?;
    validate(&workflow)?;
    Ok(workflow)
}

/// Parse a workflow from a file path.
///
/// # Errors
/// Returns [`WorkflowParseError::Io`] on read failure, otherwise the
/// same errors as [`parse_workflow_text`].
pub fn parse_workflow_file(path: &Path) -> Result<Workflow, WorkflowParseError> {
    let s = std::fs::read_to_string(path)?;
    parse_workflow_text(&s)
}

fn validate(w: &Workflow) -> Result<(), WorkflowParseError> {
    if w.workflow.name.trim().is_empty() {
        return Err(WorkflowParseError::MissingField("workflow.name"));
    }
    if w.trigger.trigger_type.trim().is_empty() {
        return Err(WorkflowParseError::MissingField("trigger.type"));
    }
    for (i, step) in w.steps.iter().enumerate() {
        if step.step_type.trim().is_empty() {
            tracing::warn!(index = i, "workflow step missing type");
            return Err(WorkflowParseError::MissingField("steps[].type"));
        }
    }
    // AIG-03 — type-specific trigger validation. Catches invalid
    // cron expressions, non-`/` webhook paths, and malformed
    // file_event regex / event lists at parse time.
    crate::trigger_validation::validate_trigger(w).map_err(WorkflowParseError::InvalidTrigger)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
[workflow]
name = "Smoke"
description = "smoke test"

[trigger]
type = "manual"

[[steps]]
type = "noop"
"#;

    #[test]
    fn parses_minimal_workflow() {
        let w = parse_workflow_text(MINIMAL).unwrap();
        assert_eq!(w.workflow.name, "Smoke");
        assert_eq!(w.trigger.trigger_type, "manual");
        assert_eq!(w.steps.len(), 1);
        assert_eq!(w.steps[0].step_type, "noop");
    }

    #[test]
    fn captures_unknown_trigger_fields_in_extra() {
        let src = r#"
[workflow]
name = "T"

[trigger]
type = "cron"
schedule = "0 9 * * *"
timezone = "UTC"
"#;
        let w = parse_workflow_text(src).unwrap();
        assert_eq!(
            w.trigger.extra.get("schedule").and_then(|v| v.as_str()),
            Some("0 9 * * *")
        );
        assert_eq!(
            w.trigger.extra.get("timezone").and_then(|v| v.as_str()),
            Some("UTC")
        );
    }

    #[test]
    fn parses_inputs_with_defaults() {
        let src = r#"
[workflow]
name = "I"

[trigger]
type = "manual"

[inputs]
dir = { type = "string", default = "journal/" }
"#;
        let w = parse_workflow_text(src).unwrap();
        let input = w.inputs.get("dir").unwrap();
        assert_eq!(input.input_type, "string");
        assert_eq!(
            input.default.as_ref().and_then(|v| v.as_str()),
            Some("journal/")
        );
    }

    #[test]
    fn rejects_empty_workflow_name() {
        let src = r#"
[workflow]
name = "   "

[trigger]
type = "manual"
"#;
        let err = parse_workflow_text(src).unwrap_err();
        assert!(matches!(
            err,
            WorkflowParseError::MissingField("workflow.name")
        ));
    }

    #[test]
    fn rejects_missing_trigger_type() {
        // TOML decode needs the type key; simulate missing via empty string.
        let src = r#"
[workflow]
name = "T"

[trigger]
type = ""
"#;
        let err = parse_workflow_text(src).unwrap_err();
        assert!(matches!(
            err,
            WorkflowParseError::MissingField("trigger.type")
        ));
    }

    #[test]
    fn parses_condition_block_with_combinators() {
        let src = r#"
[workflow]
name = "C"

[trigger]
type = "manual"

[condition]
type = "and"
conditions = [
  { type = "regex_match", source = "trigger.file.path", pattern = "notes/.*" },
  { type = "time_range", after = "09:00", before = "17:00" }
]
"#;
        let w = parse_workflow_text(src).unwrap();
        let cond = w.condition.as_ref().unwrap();
        assert_eq!(cond.condition_type, "and");
        let inner = cond
            .extra
            .get("conditions")
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(inner.len(), 2);
    }

    #[test]
    fn step_retry_fields_round_trip() {
        let src = r#"
[workflow]
name = "R"

[trigger]
type = "manual"

[[steps]]
type = "noop"
max_retries = 5
retry_backoff = "linear"
retry_initial_delay_ms = 250
retry_max_delay_ms = 5000
retry_jitter = false
"#;
        let w = parse_workflow_text(src).unwrap();
        let s = &w.steps[0];
        assert_eq!(s.max_retries, Some(5));
        assert_eq!(s.retry_backoff.as_deref(), Some("linear"));
        assert_eq!(s.retry_initial_delay_ms, Some(250));
        assert_eq!(s.retry_max_delay_ms, Some(5000));
        assert_eq!(s.retry_jitter, Some(false));

        // Re-serialize and re-parse to confirm the fields survive a
        // round trip (they should not land in `extra`).
        let out = toml::to_string(&w).unwrap();
        let w2 = parse_workflow_text(&out).unwrap();
        let s2 = &w2.steps[0];
        assert_eq!(s2.max_retries, Some(5));
        assert_eq!(s2.retry_backoff.as_deref(), Some("linear"));
        assert_eq!(s2.retry_initial_delay_ms, Some(250));
        assert_eq!(s2.retry_max_delay_ms, Some(5000));
        assert_eq!(s2.retry_jitter, Some(false));
        assert!(!s2.extra.contains_key("max_retries"));
        assert!(!s2.extra.contains_key("retry_backoff"));
    }

    #[test]
    fn parses_error_handling_block() {
        let src = r#"
[workflow]
name = "E"

[trigger]
type = "manual"

[error_handling]
max_retries = 3
retry_backoff = "exponential"
on_step_failure = "stop"
"#;
        let w = parse_workflow_text(src).unwrap();
        let eh = w.error_handling.as_ref().unwrap();
        assert_eq!(eh.max_retries, Some(3));
        assert_eq!(eh.retry_backoff.as_deref(), Some("exponential"));
        assert_eq!(eh.on_step_failure.as_deref(), Some("stop"));
    }
}
