//! Agent command handlers — `nexus agent plan|run`.
//!
//! Both drive through `com.nexus.agent` via `ipc_call`; the CLI
//! never links `nexus-agent` directly. Matches the shape of
//! `commands::ai` so agents and AI share a consistent surface.
//!
//! Per ADR 0025 Phase 1, `run` now drives `session_run`
//! (auto-approve mode) and renders the resulting transcript.
//! `run-plan` was removed — there's no session-model equivalent
//! for replaying a static plan.

use std::io::{self, BufRead, Write};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use nexus_kernel::{EventFilter, Events as _, Ipc as _, NexusEvent};
use nexus_types::constants::IPC_TIMEOUT_EXTENDED as IPC_TIMEOUT;
use nexus_types::plugin_ids;
use serde_json::Value;

use crate::app::App;
use crate::output::OutputFormat;

const AGENT_PLUGIN: &str = plugin_ids::AGENT;

/// BL-132 — bus topic prefix the interactive run subscribes to so it
/// only sees agent-emitted events.
const APPROVAL_TOPIC_PREFIX: &str = "com.nexus.agent.";

/// Plugin id of the BL-133 notifications dispatcher. Auto-notify
/// threshold is sourced from the `--notify-after-secs` flag on
/// `nexus agent run` (default 30s; 0 disables). See
/// `AgentCommand::Run` in `main.rs` for the flag definition.
const NOTIFICATIONS_PLUGIN: &str = plugin_ids::NOTIFICATIONS;

/// `nexus agent plan <goal> [--archetype ..]` — produce a plan without executing.
pub fn plan(app: &mut App, goal: &str, archetype: Option<&str>) -> Result<()> {
    let response = call(app, "plan", goal_args(goal, archetype))?;
    print_plan(&response);
    Ok(())
}

/// `nexus agent run <goal> [--archetype ..] [--interactive]` —
/// drive a session end-to-end. Without `--interactive`, every
/// round auto-approves (pre-BL-132 default). With `--interactive`,
/// drives the BL-132 approval flow: subscribe to
/// `com.nexus.agent.round_proposed` events, prompt the user (y/n on
/// stderr) for any round whose tool calls carry
/// `requires_approval = true`, and reply via the `round_decide` IPC.
pub fn run(
    app: &mut App,
    goal: &str,
    archetype: Option<&str>,
    interactive: bool,
    notify_after_secs: u64,
) -> Result<()> {
    // Capture the format before the `&mut app` borrows below; it drives
    // whether we render the human transcript or emit the raw session JSON.
    let format = app.format();
    let mut args = goal_args(goal, archetype);
    if let Some(map) = args.as_object_mut() {
        // BL-132: `auto_approve = false` engages BusBridgePolicy
        // server-side; the CLI is then responsible for handling
        // round_proposed events and replying via round_decide.
        map.insert("auto_approve".into(), Value::Bool(!interactive));
    }
    let started = Instant::now();
    let response = if interactive {
        run_interactive(app, args)?
    } else {
        call(app, "session_run", args)?
    };
    let elapsed = started.elapsed();
    if notify_after_secs > 0 && elapsed >= Duration::from_secs(notify_after_secs) {
        if let Err(err) = dispatch_completion_notification(app, goal, elapsed, &response) {
            // Notification failure is non-fatal — the session itself
            // already succeeded. Surface on stderr so the user can
            // see why their toast didn't appear, without aborting.
            eprintln!("[agent] notification dispatch failed: {err}");
        }
    }
    emit_session(format, &response);
    Ok(())
}

/// Render a completed session. `--format json` / `jsonl` emit the raw session
/// transcript (the `session_run` reply verbatim) so machine consumers — most
/// notably a headless subagent's parent process (RFC 0007) — can parse the
/// outcome; every other format renders the human-readable transcript.
fn emit_session(format: OutputFormat, response: &Value) {
    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(response).unwrap_or_default()
            );
        }
        OutputFormat::Jsonl => {
            println!("{}", serde_json::to_string(response).unwrap_or_default());
        }
        OutputFormat::Text | OutputFormat::Table => print_session(response),
    }
}

/// BL-133 follow-up — dispatch `com.nexus.notifications::send` with
/// a one-line completion summary. BL-135 reshape: emit a source-
/// tagged Notification (`source = "agent"`) instead of hardcoding
/// `channel = "desktop"`. The notifications plugin's router consults
/// `notifications.toml` to pick the channel list; users who add a
/// `[sources.agent]` block get Discord/Telegram/email delivery
/// without editing this file. Best-effort: an empty response or
/// missing fields render a sensible fallback rather than crashing.
fn dispatch_completion_notification(
    app: &mut App,
    goal: &str,
    elapsed: Duration,
    response: &Value,
) -> Result<()> {
    let summary = compose_completion_message(goal, elapsed, response);
    let args = serde_json::json!({
        "source": "agent",
        "severity": "info",
        "title": "Agent session",
        "message": summary,
    });
    let (invoker, rt) = app.invoker()?;
    rt.block_on(invoker.ipc_call(NOTIFICATIONS_PLUGIN, "send", args, IPC_TIMEOUT))
        .with_context(|| "notifications ipc call 'send' failed")?;
    Ok(())
}

/// Build the toast body. Format: `<outcome> · <round-count> rounds ·
/// <duration> · <goal-prefix>`. Truncates the goal at ~60 chars so
/// the toast doesn't balloon for long goals.
pub(crate) fn compose_completion_message(
    goal: &str,
    elapsed: Duration,
    response: &Value,
) -> String {
    let outcome = response
        .get("outcome")
        .and_then(Value::as_str)
        .unwrap_or("completed");
    let round_count = response
        .get("rounds")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let secs = elapsed.as_secs();
    let duration = if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m{}s", secs / 60, secs % 60)
    };
    let goal_prefix = if goal.len() > 60 {
        // Snap to char boundary so multi-byte codepoints aren't split.
        let mut cut = 60;
        while cut > 0 && !goal.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…", &goal[..cut])
    } else {
        goal.to_string()
    };
    format!("{outcome} · {round_count} rounds · {duration} · {goal_prefix}")
}

/// BL-132 interactive driver. Subscribes to the agent's bus topic
/// prefix, drives the `session_run` IPC concurrently via a
/// `tokio::select!` over the call's future + the bus subscription,
/// and for every `round_proposed` event with `requires_approval =
/// true` prompts the user on stderr (yes / no) and dispatches
/// `round_decide`. Rounds whose tool calls are all flagged
/// `requires_approval = false` auto-approve through the
/// `BusBridgePolicy` short-circuit and never produce a prompt.
fn run_interactive(app: &mut App, args: Value) -> Result<Value> {
    let (runtime, rt) = app.runtime()?;
    rt.block_on(async {
        let mut sub = runtime
            .context
            .subscribe(EventFilter::CustomPrefix(APPROVAL_TOPIC_PREFIX.to_owned()));
        // `session_run` is a long-lived IPC call; the CLI's bus
        // subscriber must drain events concurrently and dispatch
        // `round_decide` replies while the call is in flight.
        // `tokio::select!` over both arms keeps both alive on the
        // same task — `runtime.context` is borrowed once for the
        // outer call and re-borrowed inside the event handler for
        // each `round_decide`.
        let mut session_call = Box::pin(runtime.context.ipc_call(
            AGENT_PLUGIN,
            "session_run",
            args,
            IPC_TIMEOUT,
        ));
        loop {
            tokio::select! {
                biased;
                evt = sub.recv() => {
                    let Ok(event) = evt else { continue };
                    let NexusEvent::Custom { type_id, payload, .. } = &event.event else {
                        continue;
                    };
                    if type_id != "com.nexus.agent.round_proposed" {
                        continue;
                    }
                    let decision = match prompt_for_round(payload) {
                        PromptOutcome::Approve => true,
                        PromptOutcome::Reject => false,
                        PromptOutcome::AutoApprove => continue,
                    };
                    let Some(session_id) =
                        payload.get("session_id").and_then(Value::as_str)
                    else {
                        continue;
                    };
                    let Some(round) = payload.get("round").and_then(Value::as_u64) else {
                        continue;
                    };
                    let reply = serde_json::json!({
                        "session_id": session_id,
                        "round": round,
                        "approved": decision,
                    });
                    if let Err(e) = runtime
                        .context
                        .ipc_call(AGENT_PLUGIN, "round_decide", reply, IPC_TIMEOUT)
                        .await
                    {
                        eprintln!("[agent] round_decide failed: {e}");
                    }
                }
                result = &mut session_call => {
                    return result.with_context(|| "agent ipc call 'session_run' failed");
                }
            }
        }
    })
}

/// Outcome of presenting a `round_proposed` payload to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptOutcome {
    Approve,
    Reject,
    /// All tool calls in the round are flagged
    /// `requires_approval = false` — the bus bridge will short-circuit
    /// on its own; the CLI doesn't need to prompt or reply.
    AutoApprove,
}

/// Render the `round_proposed` payload on stderr and read a y/n
/// answer from stdin. Returns [`PromptOutcome::AutoApprove`] when the
/// payload's tool calls are all flagged
/// `requires_approval = false` so the caller can skip the round
/// entirely.
fn prompt_for_round(payload: &Value) -> PromptOutcome {
    let outcome = classify_round(payload);
    if outcome == PromptOutcome::AutoApprove {
        return outcome;
    }
    eprintln!();
    eprintln!("──[ approval required ]──────────────────────");
    if let Some(round) = payload.get("round").and_then(Value::as_u64) {
        eprintln!("round: {round}");
    }
    if let Some(text) = payload.get("text").and_then(Value::as_str) {
        if !text.is_empty() {
            eprintln!("model: {}", first_line(text));
        }
    }
    if let Some(calls) = payload.get("tool_calls").and_then(Value::as_array) {
        for (i, c) in calls.iter().enumerate() {
            let name = c.get("name").and_then(Value::as_str).unwrap_or("?");
            let requires = c
                .get("requires_approval")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let registered = c
                .get("registered")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let badge = match (requires, registered) {
                (true, true) => "DESTRUCTIVE",
                (true, false) => "UNREGISTERED",
                (false, _) => "safe",
            };
            eprintln!("  {idx}. [{badge}] {name}", idx = i + 1);
            if let Some(target) = c.get("target_plugin_id").and_then(Value::as_str) {
                let cmd = c.get("command_id").and_then(Value::as_str).unwrap_or("?");
                eprintln!("       → {target}::{cmd}");
            }
        }
    }
    eprint!("Approve? [y/N] ");
    let _ = io::stderr().flush();
    let stdin = io::stdin();
    let mut line = String::new();
    if stdin.lock().read_line(&mut line).is_err() {
        eprintln!("[reject — stdin read error]");
        return PromptOutcome::Reject;
    }
    if parse_yes(&line) {
        PromptOutcome::Approve
    } else {
        PromptOutcome::Reject
    }
}

/// Classify a `round_proposed` payload by checking whether any
/// `tool_calls[*].requires_approval` is `true`. Returns
/// [`PromptOutcome::AutoApprove`] when every call is flagged false —
/// the bus bridge auto-approves these on its own so the CLI doesn't
/// need to surface a prompt. Default `true` for unknown / missing
/// fields, matching the server-side conservative default.
fn classify_round(payload: &Value) -> PromptOutcome {
    let Some(calls) = payload.get("tool_calls").and_then(Value::as_array) else {
        // No tool calls — treat as auto-approve; the bus bridge will
        // short-circuit too.
        return PromptOutcome::AutoApprove;
    };
    let any_destructive = calls.iter().any(|c| {
        c.get("requires_approval")
            .and_then(Value::as_bool)
            .unwrap_or(true)
    });
    if any_destructive {
        PromptOutcome::Reject // placeholder — the actual decision
                              // comes from the user prompt in
                              // `prompt_for_round`. Returning
                              // anything other than `AutoApprove`
                              // here is fine; the caller only checks
                              // for `== AutoApprove`.
    } else {
        PromptOutcome::AutoApprove
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or(s)
}

/// Parse a user reply line as yes/no. Accepts `y`, `yes`, `Y`,
/// `YES`, leading whitespace OK. Anything else — including empty
/// (just enter) — counts as no.
fn parse_yes(line: &str) -> bool {
    let trimmed = line.trim().to_ascii_lowercase();
    matches!(trimmed.as_str(), "y" | "yes")
}

/// `nexus agent list-custom` — list `.agent.toml` manifests under
/// `<forge>/.forge/agents/`. PRD-15 §9 (DG-36).
///
/// # Errors
/// Surfaces IPC dispatch errors verbatim.
pub fn list_custom(app: &mut App) -> Result<()> {
    let response = call(app, "list_custom", Value::Object(serde_json::Map::new()))?;
    let manifests = response
        .get("manifests")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let errors = response
        .get("errors")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if manifests.is_empty() && errors.is_empty() {
        println!("(no custom agents under .forge/agents/)");
        return Ok(());
    }

    if !manifests.is_empty() {
        let slug_w = manifests
            .iter()
            .filter_map(|m| m.get("slug").and_then(Value::as_str))
            .map(str::len)
            .max()
            .unwrap_or(4)
            .max("SLUG".len());
        println!(
            "{:width$}  NAME                 ARCHETYPE",
            "SLUG",
            width = slug_w
        );
        for m in &manifests {
            let slug = m.get("slug").and_then(Value::as_str).unwrap_or("?");
            let name = m
                .get("agent")
                .and_then(|a| a.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("(unnamed)");
            let arche = m
                .get("agent")
                .and_then(|a| a.get("archetype"))
                .and_then(Value::as_str)
                .unwrap_or("-");
            println!("{slug:width$}  {name:<20} {arche}", width = slug_w);
        }
    }
    if !errors.is_empty() {
        if !manifests.is_empty() {
            println!();
        }
        println!("errors ({}):", errors.len());
        for e in errors {
            let path = e.get("path").and_then(Value::as_str).unwrap_or("?");
            let msg = e.get("error").and_then(Value::as_str).unwrap_or("?");
            println!("  {path}: {msg}");
        }
    }
    Ok(())
}

fn goal_args(goal: &str, archetype: Option<&str>) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("goal".into(), Value::String(goal.into()));
    if let Some(a) = archetype {
        map.insert("archetype".into(), Value::String(a.into()));
    }
    Value::Object(map)
}

// ── Printers ────────────────────────────────────────────────────────────────

fn print_plan(plan: &Value) {
    let id = plan.get("id").and_then(Value::as_str).unwrap_or("<no-id>");
    let goal = plan
        .get("goal")
        .and_then(Value::as_str)
        .unwrap_or("<no-goal>");
    let steps = plan
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    println!("Plan  : {id}");
    println!("Goal  : {goal}");
    println!("Steps : {}", steps.len());
    for (i, step) in steps.iter().enumerate() {
        let desc = step
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("<no-description>");
        let sid = step.get("id").and_then(Value::as_str).unwrap_or("");
        print!("  {}. [{sid}] {desc}", i + 1);
        if let Some(call) = step.get("tool_call").filter(|v| !v.is_null()) {
            let target = call
                .get("target_plugin_id")
                .and_then(Value::as_str)
                .unwrap_or("?");
            let cmd = call
                .get("command_id")
                .and_then(Value::as_str)
                .unwrap_or("?");
            print!(" → {target}.{cmd}");
        }
        println!();
    }
}

fn print_session(session: &Value) {
    let id = session
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("<no-id>");
    let goal = session
        .get("goal")
        .and_then(Value::as_str)
        .unwrap_or("<no-goal>");
    let outcome = session
        .get("outcome")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let rounds = session
        .get("rounds")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    println!("Session : {id}");
    println!("Goal    : {goal}");
    println!("Outcome : {outcome}");
    println!("Rounds  : {}", rounds.len());
    for round in &rounds {
        let n = round.get("round").and_then(Value::as_u64).unwrap_or(0);
        let text = round.get("text").and_then(Value::as_str).unwrap_or("");
        let calls = round
            .get("tool_calls")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        println!("  round {n}");
        if !text.is_empty() {
            for line in text.lines() {
                println!("    {line}");
            }
        }
        for tc in &calls {
            let name = tc.get("name").and_then(Value::as_str).unwrap_or("?");
            let approved = tc.get("approved").and_then(Value::as_bool).unwrap_or(false);
            let error = tc.get("error").and_then(Value::as_str).unwrap_or("");
            let marker = if !error.is_empty() {
                "✗"
            } else if approved {
                "✓"
            } else {
                "·"
            };
            print!("    {marker} {name}");
            if !error.is_empty() {
                print!(" — {error}");
            } else if let Some(resp) = tc.get("response").filter(|v| !v.is_null()) {
                let preview = preview_json(resp, 160);
                print!(" — {preview}");
            }
            println!();
        }
    }
}

fn preview_json(v: &Value, max: usize) -> String {
    let full = v.to_string();
    if full.len() <= max {
        full
    } else {
        let mut end = max;
        while !full.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}…", &full[..end])
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn call(app: &mut App, command: &str, args: Value) -> Result<Value> {
    let (invoker, rt) = app.invoker()?;
    rt.block_on(invoker.ipc_call(AGENT_PLUGIN, command, args, IPC_TIMEOUT))
        .with_context(|| format!("agent ipc call '{command}' failed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── BL-132 — interactive prompt helpers ──────────────────────

    #[test]
    fn parse_yes_accepts_y_and_yes_case_insensitive() {
        assert!(parse_yes("y\n"));
        assert!(parse_yes("Y\n"));
        assert!(parse_yes("yes\n"));
        assert!(parse_yes("  YES  \n"));
        assert!(parse_yes("yes"));
    }

    #[test]
    fn parse_yes_rejects_anything_else() {
        assert!(!parse_yes("\n"));
        assert!(!parse_yes(""));
        assert!(!parse_yes("n\n"));
        assert!(!parse_yes("no\n"));
        assert!(!parse_yes("maybe\n"));
        assert!(!parse_yes("approve\n"));
    }

    #[test]
    fn classify_round_no_tool_calls_returns_auto_approve() {
        let payload = serde_json::json!({ "session_id": "s", "round": 1 });
        assert_eq!(classify_round(&payload), PromptOutcome::AutoApprove);
    }

    #[test]
    fn classify_round_all_safe_tools_returns_auto_approve() {
        let payload = serde_json::json!({
            "session_id": "s",
            "round": 1,
            "tool_calls": [
                { "name": "read_file", "requires_approval": false, "registered": true },
                { "name": "search_forge", "requires_approval": false, "registered": true },
            ],
        });
        assert_eq!(classify_round(&payload), PromptOutcome::AutoApprove);
    }

    #[test]
    fn classify_round_any_destructive_tool_skips_auto_approve() {
        let payload = serde_json::json!({
            "session_id": "s",
            "round": 1,
            "tool_calls": [
                { "name": "read_file", "requires_approval": false, "registered": true },
                { "name": "write_file", "requires_approval": true, "registered": true },
            ],
        });
        // The exact non-AutoApprove value is irrelevant — the caller
        // only checks `== AutoApprove` before proceeding to prompt.
        // Asserting != AutoApprove pins the contract without tying
        // the test to the placeholder value.
        assert_ne!(classify_round(&payload), PromptOutcome::AutoApprove);
    }

    #[test]
    fn classify_round_unregistered_tool_defaults_to_destructive() {
        // The agent's server-side default for unknown tools is
        // `requires_approval = true`. The CLI mirrors that
        // conservative stance: if the field is missing, treat as
        // destructive.
        let payload = serde_json::json!({
            "session_id": "s",
            "round": 1,
            "tool_calls": [
                { "name": "unregistered_thing" },
            ],
        });
        assert_ne!(classify_round(&payload), PromptOutcome::AutoApprove);
    }

    #[test]
    fn first_line_returns_first_segment_only() {
        assert_eq!(first_line("hello\nworld"), "hello");
        assert_eq!(first_line("single"), "single");
        assert_eq!(first_line(""), "");
    }

    // ── BL-133 follow-up — agent-completion message composer ────

    #[test]
    fn compose_completion_message_short_run_under_a_minute() {
        let goal = "summarise yesterday's notes";
        let elapsed = Duration::from_secs(35);
        let response = serde_json::json!({
            "outcome": "complete",
            "rounds": [{ "round": 1 }, { "round": 2 }, { "round": 3 }],
        });
        let msg = compose_completion_message(goal, elapsed, &response);
        assert_eq!(
            msg,
            "complete · 3 rounds · 35s · summarise yesterday's notes"
        );
    }

    #[test]
    fn compose_completion_message_minute_or_more_renders_mmss() {
        let elapsed = Duration::from_secs(125); // 2m05s
        let response = serde_json::json!({
            "outcome": "max_rounds",
            "rounds": [{}, {}, {}, {}, {}],
        });
        let msg = compose_completion_message("x", elapsed, &response);
        assert!(msg.contains("2m5s"), "expected 2m5s in: {msg}");
        assert!(msg.contains("max_rounds"));
        assert!(msg.contains("5 rounds"));
    }

    #[test]
    fn compose_completion_message_truncates_long_goal() {
        let goal = "a".repeat(200);
        let response = serde_json::json!({ "outcome": "complete", "rounds": [] });
        let msg = compose_completion_message(&goal, Duration::from_secs(31), &response);
        assert!(msg.contains("…"));
        // 60 chars of goal + 1 ellipsis + the rest of the format —
        // total still well under any sane toast cap.
        assert!(msg.len() < 200, "toast body too long: {msg}");
    }

    #[test]
    fn compose_completion_message_utf8_safe_truncation() {
        // Goal padded with multi-byte emoji so a naive byte cut at
        // 60 would land mid-codepoint. The composer snaps back to a
        // char boundary; this test panics if the slice would.
        let goal = format!("{} more text here for padding", "🦀".repeat(20));
        let response = serde_json::json!({ "outcome": "complete", "rounds": [] });
        let _msg = compose_completion_message(&goal, Duration::from_secs(31), &response);
        // No panic → success. (The boundary snap happens inside
        // compose_completion_message; this test pins the contract.)
    }

    #[test]
    fn compose_completion_message_handles_missing_fields() {
        let response = serde_json::json!({});
        let msg = compose_completion_message("hi", Duration::from_secs(31), &response);
        // Falls back to "completed" + 0 rounds + duration + goal.
        assert!(msg.starts_with("completed · 0 rounds · 31s · hi"));
    }
}
