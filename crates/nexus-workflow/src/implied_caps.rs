//! A5 (2026-05-21 audit) — implied-capability computation for
//! workflows.
//!
//! `com.nexus.workflow::run` and `::run_digest` are classified as
//! `unrestricted` in the cap matrix: any caller with `ipc.call` can
//! dispatch them. The workflow plugin's own [`KernelPluginContext`]
//! holds a minimal cap set (`IpcCall`, `AiChat`, `AiRuntimeSubmit`)
//! — see [`nexus_bootstrap::workflow_capabilities`]. Each step the
//! executor dispatches is checked against THOSE caps, not against
//! the caller of `run`. The net effect is laundering: a caller
//! with no `ai.chat` cap can drive an `ai_prompt` step that
//! transitively reaches `com.nexus.ai::ask` — gaining the workflow
//! plugin's `AiChat` cap through composition. Issue #77.
//!
//! This module computes the **implied capability surface** of a
//! parsed [`crate::Workflow`]: for each step type the executor
//! handles in [`crate::handlers::run`], we know which kernel
//! handler it ultimately reaches and which caps that handler
//! requires. Walking the steps and unioning those caps yields the
//! set the caller of `run` *would* need to hold if the kernel
//! enforced caller-cap parity through the dispatch chain.
//!
//! Consumer today: the run handler emits a `tracing::warn!(audit
//! = true, …)` at workflow run entry with the implied set so
//! operators can see the laundering surface in audit logs even
//! without kernel-level enforcement.
//!
//! Kernel-level enforcement (the actual fix that would require the
//! caller to hold the union of implied caps) is the residual gap
//! tracked under issue #77 — it needs the cap-policy mechanism in
//! `nexus-bootstrap::cap_policies` to be made forge-root-aware so a
//! policy closure can read the workflow file on dispatch. The
//! [`validate_declared_caps`] helper is shipped now as a building
//! block for that follow-up: once workflows carry an
//! author-declared cap list, this function verifies the
//! declaration covers the computed implied set.

use crate::{Step, Workflow};

/// Capabilities that a workflow's steps would transitively
/// require if the kernel enforced caller-cap parity through the
/// dispatch chain. Returned as a sorted, de-duplicated `Vec<&'static str>`
/// using the kernel's stringified `Capability::as_str()` form (so
/// the result composes cleanly with `Capability::ALL` and with
/// `audit-flags.md`).
///
/// Step types map as follows. The mapping mirrors the actual
/// dispatch performed by [`crate::handlers::run::KernelActionDispatcher`]:
///
/// | Step type             | Reaches                               | Implied caps  |
/// |-----------------------|---------------------------------------|---------------|
/// | `noop`                | (nothing)                             | (none)        |
/// | `ipc` / `ipc_call`    | `step.extra.target::command`          | unknown (\*)  |
/// | `ai_prompt`           | `com.nexus.ai::ask`                   | `ai.chat`     |
/// | `ai_decision`         | `com.nexus.ai::ask`                   | `ai.chat`     |
/// | `terminal`            | `com.nexus.terminal::run_saved`/etc.  | `process.spawn` |
/// | `notify`              | `com.nexus.notifications::send`       | (none extra)  |
/// | (any other)           | swallowed as no-op by the executor    | (none)        |
///
/// (\*) `ipc_call` steps can target any plugin/command — the cap
/// surface depends on the matrix entry for that target. We surface
/// the target so operators can audit it; the implied cap can't be
/// computed without a matrix lookup, which lives in
/// `nexus-bootstrap`. The free-form target is logged for
/// observability and the audit comment notes the limitation.
#[must_use]
pub fn compute_implied_caps(workflow: &Workflow) -> ImpliedCaps {
    let mut caps: Vec<&'static str> = Vec::new();
    let mut ipc_targets: Vec<String> = Vec::new();
    for step in &workflow.steps {
        match step.step_type.as_str() {
            "ai_prompt" | "ai_decision" => caps.push("ai.chat"),
            "terminal" => caps.push("process.spawn"),
            "ipc" | "ipc_call" => {
                if let Some(target) = ipc_step_target(step) {
                    ipc_targets.push(target);
                }
            }
            // `noop`, `notify`, and unknown types contribute nothing
            // (notify currently has no extra cap on its target;
            // unknown types are no-op'd by the executor).
            _ => {}
        }
    }
    caps.sort_unstable();
    caps.dedup();
    ipc_targets.sort();
    ipc_targets.dedup();
    ImpliedCaps { caps, ipc_targets }
}

/// Extract `<target_plugin>::<command>` for an `ipc` step so the
/// audit log lists each free-form target the workflow can reach.
/// Mirrors the same `extra.get(...).as_str()` pattern that
/// [`crate::handlers::run::KernelActionDispatcher`] uses to
/// dispatch — anything that the dispatcher can't pull out a
/// `target` for is silently skipped here too.
fn ipc_step_target(step: &Step) -> Option<String> {
    let target = step.extra.get("target").and_then(|v| v.as_str())?;
    let command = step.extra.get("command").and_then(|v| v.as_str())?;
    Some(format!("{target}::{command}"))
}

/// Result of [`compute_implied_caps`]. `caps` is the union of
/// statically-known caps; `ipc_targets` is the set of free-form
/// `ipc_call` step targets whose cap surface can't be computed
/// without a matrix lookup.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImpliedCaps {
    /// Statically-known caps the workflow's steps would require if
    /// the kernel enforced caller-cap parity through the dispatch
    /// chain. Sorted, de-duplicated, using the
    /// `Capability::as_str()` form.
    pub caps: Vec<&'static str>,
    /// Free-form `<plugin>::<command>` strings for every `ipc` /
    /// `ipc_call` step. Sorted, de-duplicated. Their cap surface
    /// is not statically known here (would require a matrix
    /// lookup), so they are surfaced separately for audit logs.
    pub ipc_targets: Vec<String>,
}

impl ImpliedCaps {
    /// True iff no statically-known caps are implied AND there are
    /// no free-form ipc targets — i.e. the workflow's laundering
    /// surface is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.caps.is_empty() && self.ipc_targets.is_empty()
    }
}

/// A5 follow-up — compare an author-declared `required_caps` list
/// against the computed implied set. Returns `Ok(())` when the
/// declaration covers every statically-known cap; returns an `Err`
/// listing the caps the workflow needs but the author didn't
/// declare. `ipc_targets` are not validated here (their cap
/// requirement is not statically known); callers that want to be
/// strict about ipc targets can add a separate check downstream.
///
/// This is a building block for the issue-#77 follow-up that adds
/// `[workflow].required_caps` to the file schema — shipped now so
/// the validation logic can be tested independently of the schema
/// change.
///
/// # Errors
///
/// Returns `Err` with the missing caps (sorted, no duplicates)
/// when at least one statically-known cap is absent from
/// `declared`.
pub fn validate_declared_caps(
    declared: &[String],
    implied: &ImpliedCaps,
) -> Result<(), Vec<&'static str>> {
    let missing: Vec<&'static str> = implied
        .caps
        .iter()
        .copied()
        .filter(|cap| !declared.iter().any(|d| d == cap))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Step, Trigger, Workflow, WorkflowMeta};
    use std::collections::BTreeMap;

    fn mk_workflow(steps: Vec<Step>) -> Workflow {
        Workflow {
            workflow: WorkflowMeta {
                name: "t".into(),
                description: String::new(),
                version: String::new(),
                author: String::new(),
                tags: Vec::new(),
                extra: BTreeMap::new(),
            },
            inputs: BTreeMap::new(),
            trigger: Trigger {
                trigger_type: "manual".into(),
                extra: BTreeMap::new(),
            },
            condition: None,
            steps,
            outputs: BTreeMap::new(),
            error_handling: None,
            extra: BTreeMap::new(),
        }
    }

    fn mk_step(step_type: &str) -> Step {
        Step {
            name: None,
            step_type: step_type.into(),
            parallel: false,
            async_submit: false,
            on_error: None,
            max_retries: None,
            retry_backoff: None,
            retry_initial_delay_ms: None,
            retry_max_delay_ms: None,
            retry_jitter: None,
            extra: BTreeMap::new(),
        }
    }

    #[test]
    fn empty_workflow_implies_nothing() {
        let wf = mk_workflow(vec![]);
        let implied = compute_implied_caps(&wf);
        assert!(implied.is_empty());
    }

    #[test]
    fn ai_prompt_step_implies_ai_chat() {
        let wf = mk_workflow(vec![mk_step("ai_prompt")]);
        let implied = compute_implied_caps(&wf);
        assert_eq!(implied.caps, vec!["ai.chat"]);
        assert!(implied.ipc_targets.is_empty());
    }

    #[test]
    fn ai_decision_step_also_implies_ai_chat() {
        let wf = mk_workflow(vec![mk_step("ai_decision")]);
        let implied = compute_implied_caps(&wf);
        assert_eq!(implied.caps, vec!["ai.chat"]);
    }

    #[test]
    fn terminal_step_implies_process_spawn() {
        let wf = mk_workflow(vec![mk_step("terminal")]);
        let implied = compute_implied_caps(&wf);
        assert_eq!(implied.caps, vec!["process.spawn"]);
    }

    #[test]
    fn multiple_step_types_union_and_dedupe() {
        let wf = mk_workflow(vec![
            mk_step("ai_prompt"),
            mk_step("terminal"),
            mk_step("ai_decision"),
            mk_step("noop"),
            mk_step("ai_prompt"),
        ]);
        let implied = compute_implied_caps(&wf);
        assert_eq!(implied.caps, vec!["ai.chat", "process.spawn"]);
    }

    #[test]
    fn ipc_step_target_recorded_separately() {
        let mut step = mk_step("ipc");
        step.extra.insert(
            "target".into(),
            toml::Value::String("com.nexus.git".into()),
        );
        step.extra
            .insert("command".into(), toml::Value::String("push".into()));
        let wf = mk_workflow(vec![step]);
        let implied = compute_implied_caps(&wf);
        assert!(implied.caps.is_empty());
        assert_eq!(implied.ipc_targets, vec!["com.nexus.git::push"]);
    }

    #[test]
    fn ipc_step_targets_dedupe_and_sort() {
        let mk_ipc = |target: &str, command: &str| {
            let mut s = mk_step("ipc");
            s.extra
                .insert("target".into(), toml::Value::String(target.into()));
            s.extra
                .insert("command".into(), toml::Value::String(command.into()));
            s
        };
        let mut s_call = mk_ipc("com.nexus.ai", "stream_chat");
        s_call.step_type = "ipc_call".into();
        let wf = mk_workflow(vec![
            mk_ipc("com.nexus.git", "push"),
            s_call,
            mk_ipc("com.nexus.git", "push"),
        ]);
        let implied = compute_implied_caps(&wf);
        assert_eq!(
            implied.ipc_targets,
            vec!["com.nexus.ai::stream_chat", "com.nexus.git::push"]
        );
    }

    #[test]
    fn unknown_step_type_implies_nothing() {
        let wf = mk_workflow(vec![mk_step("not_a_real_step_type")]);
        let implied = compute_implied_caps(&wf);
        assert!(implied.is_empty());
    }

    #[test]
    fn notify_step_implies_no_extra_cap() {
        let wf = mk_workflow(vec![mk_step("notify")]);
        let implied = compute_implied_caps(&wf);
        assert!(implied.is_empty());
    }

    #[test]
    fn validate_declared_caps_passes_when_complete() {
        let wf = mk_workflow(vec![mk_step("ai_prompt"), mk_step("terminal")]);
        let implied = compute_implied_caps(&wf);
        let declared = vec!["ai.chat".to_string(), "process.spawn".to_string()];
        assert!(validate_declared_caps(&declared, &implied).is_ok());
    }

    #[test]
    fn validate_declared_caps_passes_when_superset() {
        let wf = mk_workflow(vec![mk_step("ai_prompt")]);
        let implied = compute_implied_caps(&wf);
        let declared = vec![
            "ai.chat".to_string(),
            "process.spawn".to_string(),
            "net.http".to_string(),
        ];
        assert!(validate_declared_caps(&declared, &implied).is_ok());
    }

    #[test]
    fn validate_declared_caps_lists_missing() {
        let wf = mk_workflow(vec![mk_step("ai_prompt"), mk_step("terminal")]);
        let implied = compute_implied_caps(&wf);
        let declared = vec!["ai.chat".to_string()];
        let err = validate_declared_caps(&declared, &implied).unwrap_err();
        assert_eq!(err, vec!["process.spawn"]);
    }

    #[test]
    fn validate_declared_caps_lists_all_missing_when_empty() {
        let wf = mk_workflow(vec![mk_step("ai_prompt"), mk_step("terminal")]);
        let implied = compute_implied_caps(&wf);
        let declared: Vec<String> = Vec::new();
        let err = validate_declared_caps(&declared, &implied).unwrap_err();
        assert_eq!(err, vec!["ai.chat", "process.spawn"]);
    }
}
