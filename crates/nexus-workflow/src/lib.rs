//! Workflow subsystem scaffold (PRD-16).
//!
//! Declarative pipelines — triggers, conditions, actions — composed
//! into `.workflow.toml` files living under `{forge}/.workflows/`.
//! This crate provides the *pure-logic* pieces: typed model, TOML
//! parser, and directory-walk registry. A runtime (trigger engine,
//! condition evaluator, action executor) lands as a sibling layer
//! once the shape stabilizes.
//!
//! The library is kernel-free; a `com.nexus.workflow` core plugin
//! wraps it behind `ipc_call` in a follow-up, matching the posture
//! of `nexus-skills` and `nexus-agent`.
//!
//! # What this is NOT (yet)
//!
//! - A trigger engine. Cron / fs-watcher / webhook / git-hook entry
//!   points are PRD-16 §5; they sit on top of this model.
//! - A condition evaluator. [`Condition`] round-trips through TOML
//!   but nothing evaluates it.
//! - An action executor. [`Step`] keeps per-action fields as a loose
//!   map so callers can dispatch by `type` once the executor exists.
//! - A CLI. `nexus workflow list|show|validate` will wrap the
//!   registry once the plugin surface is ready.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod ai_steps;
mod condition;
pub mod core_plugin;
pub mod cron;
pub mod digests;
mod executor;
mod interpolate;
mod parse;
mod registry;
pub mod templates;
mod trigger_validation;
pub mod webhook;

pub use ai_steps::{
    build_decision_prompt, pick_choice, AiDecisionArgs, AiPromptArgs,
};
pub use condition::{evaluate_condition, ConditionError, EvaluationContext};
pub use core_plugin::{
    WorkflowCorePlugin, HANDLER_GET, HANDLER_LIST, HANDLER_RELOAD, HANDLER_RUN,
    HANDLER_RUN_DIGEST, HANDLER_SET_DIGEST_CONFIG, HANDLER_VALIDATE, PLUGIN_ID,
};
pub use digests::{
    build_digest_prompt, digest_window, next_fire, output_path, run_digest, DigestConfig,
    DigestKind, DigestRunReport, DEFAULT_DAILY_CRON, DEFAULT_DIGESTS_DIR, DEFAULT_WEEKLY_CRON,
};
pub use cron::{next_fire_after, CronParseError, CronSchedule};
pub use executor::{
    condition_skipped_run, run_workflow, run_workflow_with_variables, ActionDispatcher,
    StepOutcome, StepOutcomeStatus, WorkflowExecutionError, WorkflowRun,
};
pub use interpolate::{interpolate_step, substitute, substitute_string, VariableMap};
pub use parse::{parse_workflow_file, parse_workflow_text, WorkflowParseError};
pub use registry::{WorkflowRegistry, WorkflowRegistryError};
pub use trigger_validation::validate_trigger;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Top-level parsed `.workflow.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workflow {
    /// `[workflow]` table — metadata about the workflow itself.
    pub workflow: WorkflowMeta,
    /// `[inputs]` table — parameter declarations. Keys are input
    /// names; values carry `type` / `default` / `description`.
    #[serde(default)]
    pub inputs: BTreeMap<String, Input>,
    /// `[trigger]` table — what causes this workflow to fire.
    pub trigger: Trigger,
    /// `[condition]` table — optional gate evaluated at trigger time.
    #[serde(default)]
    pub condition: Option<Condition>,
    /// `[[steps]]` array — ordered actions.
    #[serde(default)]
    pub steps: Vec<Step>,
    /// `[outputs]` table — exposed values for callers / downstream
    /// workflows. Kept opaque for now.
    #[serde(default)]
    pub outputs: BTreeMap<String, toml::Value>,
    /// `[error_handling]` table — retry + branch policy.
    #[serde(default)]
    pub error_handling: Option<ErrorHandling>,
    /// Everything else in the root table — preserved so forward-
    /// compat sections (e.g. `[schedule]` additions) round-trip.
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

/// `[workflow]` metadata block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowMeta {
    /// Human-readable name.
    pub name: String,
    /// One-to-two-sentence description.
    #[serde(default)]
    pub description: String,
    /// Semver string.
    #[serde(default)]
    pub version: String,
    /// Author identifier (email / handle).
    #[serde(default)]
    pub author: String,
    /// Free-form tags for discovery.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Everything else in `[workflow]`.
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

/// One entry in `[inputs]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Input {
    /// Rough type label: `string` / `number` / `bool` / `path` / etc.
    #[serde(rename = "type", default = "default_input_type")]
    pub input_type: String,
    /// Default value when the caller doesn't supply one.
    #[serde(default)]
    pub default: Option<toml::Value>,
    /// Short description for UI.
    #[serde(default)]
    pub description: Option<String>,
    /// Remaining fields (e.g. `required`, `choices`).
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

fn default_input_type() -> String {
    "string".into()
}

/// Trigger table. `type` selects the variant; remaining keys are
/// parked in `extra` — a runtime dispatches off `trigger_type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trigger {
    /// Trigger kind — `cron`, `file_event`, `manual`, `webhook`, etc.
    #[serde(rename = "type")]
    pub trigger_type: String,
    /// All other keys on the trigger table.
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

/// Condition table. Same shape as [`Trigger`] — type-dispatched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    /// Condition kind — `file_exists` / `regex_match` / `and` / etc.
    #[serde(rename = "type")]
    pub condition_type: String,
    /// All other keys on the condition table (including `conditions`
    /// sub-array for `and` / `or` combinators).
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

/// One `[[steps]]` entry. Kept as a loose map so each action type
/// can carry its own fields without forcing a monolithic enum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    /// Optional step name — referenced by downstream steps as
    /// `${StepName.output}`.
    #[serde(default)]
    pub name: Option<String>,
    /// Action type — `file_create` / `db_update` / `terminal_run` /
    /// `ai_chat` / `notification` / etc.
    #[serde(rename = "type")]
    pub step_type: String,
    /// Whether this step runs concurrently with its siblings.
    #[serde(default)]
    pub parallel: bool,
    /// On-error policy for this step — `stop` / `continue` /
    /// `log_warn` / `branch_to_recovery`.
    #[serde(default)]
    pub on_error: Option<String>,
    /// Max retries on failure for this step. `0` (default) means a
    /// single attempt. Shadows workflow-level
    /// `error_handling.max_retries` when set.
    #[serde(default)]
    pub max_retries: Option<u32>,
    /// Backoff curve — `"constant"` / `"linear"` / `"exponential"`.
    /// Defaults to `"exponential"`. Shadows workflow-level
    /// `error_handling.retry_backoff` when set.
    #[serde(default)]
    pub retry_backoff: Option<String>,
    /// Initial delay between attempts in milliseconds. Defaults to
    /// `100`.
    #[serde(default)]
    pub retry_initial_delay_ms: Option<u64>,
    /// Cap on the per-attempt delay in milliseconds. Defaults to
    /// `30_000` (30 s).
    #[serde(default)]
    pub retry_max_delay_ms: Option<u64>,
    /// Apply full-jitter to each computed delay. Defaults to `true`.
    #[serde(default)]
    pub retry_jitter: Option<bool>,
    /// All other per-step fields — `path`, `content`, `query`, etc.
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

/// `[error_handling]` block.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ErrorHandling {
    /// Max retries per step.
    #[serde(default)]
    pub max_retries: Option<u32>,
    /// `constant` / `linear` / `exponential`.
    #[serde(default)]
    pub retry_backoff: Option<String>,
    /// `stop` / `continue` / `branch_to_recovery`.
    #[serde(default)]
    pub on_step_failure: Option<String>,
    /// Named recovery step if `on_step_failure = "branch_to_recovery"`.
    #[serde(default)]
    pub recovery_step: Option<String>,
    /// Everything else.
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}
