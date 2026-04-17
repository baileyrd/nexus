//! Pre-command execution pipeline (PRD-09 §4.4).
//!
//! # Role
//!
//! Drives a [`ManagedProcess`] through its configured `pre_commands`
//! against a running [`TerminalServer`] session: writes each command
//! into the shell, waits for a sentinel line that carries the exit
//! code, and advances the FSM step-by-step. On failure or timeout the
//! machine parks in [`ManagedState::Stopped`] — matching the §4.3 rule
//! that pre-command errors abort the whole chain.
//!
//! # Microkernel fit
//!
//! Plain library glue. No kernel bus, no plugin IPC: takes a
//! `&mut dyn TerminalServer` so the core plugin can pass its own
//! `InMemoryTerminalServer` without this module caring where the
//! session lives. Keeps §4.4 execution logic testable in isolation.
//!
//! # Why a sentinel, not a subprocess
//!
//! PRD §4.4 requires pre-commands to run *in the main shell* so
//! `cd` / `source` / `export` state is inherited by the main command.
//! Spawning each step as its own child would lose that state. The
//! trade-off: we can't read an OS-reported exit code, so each step is
//! wrapped with `; printf '<sentinel> %d\\n' $?` and we scan the line
//! buffer for the sentinel to recover the code. The sentinel is
//! UUID-suffixed per run so a stray literal in command output never
//! masquerades as a completion signal.
//!
//! # What this is NOT
//!
//! - Cross-shell. Targets POSIX shells (bash, zsh, sh, dash, fish with
//!   minor quirks documented below). `cmd.exe` / `pwsh` need their own
//!   sentinel syntax — a follow-up when PRD-09 §1.2's Windows
//!   detection lands.
//! - Asynchronous. Each step runs sequentially and blocks until it
//!   finishes or its per-step timeout elapses. The caller controls
//!   concurrency by scheduling the whole pipeline on its own task.
//! - A memory / CPU limiter. §7 polling is a sibling concern.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::TerminalError;
use crate::procmgr::{ManagedProcess, TransitionError};
use crate::server::TerminalServer;
use crate::session::SessionId;

/// Default per-step timeout — PRD-09 §4.3 "30 s (user-configurable)".
pub const DEFAULT_STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// Tunables for [`run_pre_commands`]. All fields have sensible defaults
/// via [`Self::default`]; callers override only the knobs they need.
#[derive(Debug, Clone)]
pub struct PreCommandOptions {
    /// Hard deadline for each pre-command. A step that doesn't produce
    /// its sentinel within this window is treated as a timeout.
    pub step_timeout: Duration,
    /// Budget spent blocking in one [`TerminalServer::pump`] pass
    /// between sentinel checks. 100 ms matches the §5.4 polling
    /// cadence and keeps the loop from burning CPU on idle sessions.
    pub pump_interval: Duration,
}

impl Default for PreCommandOptions {
    fn default() -> Self {
        Self {
            step_timeout: DEFAULT_STEP_TIMEOUT,
            pump_interval: Duration::from_millis(100),
        }
    }
}

/// Why a pre-command pipeline finished. Isolated from [`TerminalError`]
/// because these are *expected* outcomes — the caller routes each
/// variant into a different FSM transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreCommandOutcome {
    /// Every configured pre-command exited 0. The FSM is parked on
    /// the last `PreCommand` step; caller follows up with
    /// [`ManagedProcess::mark_starting`].
    AllSucceeded,
    /// No pre-commands were configured — nothing to do.
    Skipped,
    /// A step exited non-zero. FSM is back in `Stopped`.
    StepFailed {
        /// Which step failed (0-indexed).
        step: usize,
        /// The shell's reported exit code.
        exit_code: i32,
    },
    /// A step did not produce its sentinel line within the per-step
    /// timeout. FSM is back in `Stopped`.
    StepTimedOut {
        /// Which step timed out (0-indexed).
        step: usize,
    },
}

impl PreCommandOutcome {
    /// Whether the whole chain finished cleanly.
    #[must_use]
    pub const fn is_success(&self) -> bool {
        matches!(self, PreCommandOutcome::AllSucceeded | PreCommandOutcome::Skipped)
    }
}

/// Run the `pre_commands` chain on `process` against the open session
/// `session_id`. Advances [`ManagedProcess`] one `PreCommand { step }`
/// transition per command; on any failure the machine is returned to
/// `Stopped` so the caller can decide whether to re-run, edit, or give
/// up. On full success the caller should follow up with
/// [`ManagedProcess::mark_starting`].
///
/// # Errors
/// Propagates [`TerminalError`] from the underlying
/// `send_input` / `pump` / `search_output` calls, plus an internal
/// carrier for [`TransitionError`] if the caller hands in a process
/// whose state isn't legal for pre-command entry.
pub fn run_pre_commands<S: TerminalServer>(
    server: &mut S,
    session_id: &SessionId,
    process: &mut ManagedProcess,
    options: &PreCommandOptions,
) -> Result<PreCommandOutcome, TerminalError> {
    let steps = process.config().pre_commands.clone();
    if steps.is_empty() {
        return Ok(PreCommandOutcome::Skipped);
    }

    // One sentinel prefix per run so multiple concurrent pipelines
    // (different sessions) never cross-match. We don't need the full
    // UUID form; a 12-char hex is plenty.
    let run_tag = uuid::Uuid::new_v4().simple().to_string();

    for (idx, cmd) in steps.iter().enumerate() {
        process
            .mark_pre_command(idx)
            .map_err(transition_err)?;

        let sentinel = format!("__nexus_precmd_{run_tag}_{idx}");
        // `; printf ... $?` is idempotent across dash/bash/zsh/ksh.
        // Fish does not support `$?`; fish users should run a
        // POSIX-shell subshell (`bash -c …`) for pre-commands.
        let wrapped = format!("{cmd}\nprintf '{sentinel} %d\\n' $?\n");
        server.send_raw_input(session_id, wrapped.as_bytes())?;

        match wait_for_sentinel(server, session_id, &sentinel, options)? {
            Some(0) => continue,
            Some(code) => {
                process.mark_stopped();
                return Ok(PreCommandOutcome::StepFailed {
                    step: idx,
                    exit_code: code,
                });
            }
            None => {
                process.mark_stopped();
                return Ok(PreCommandOutcome::StepTimedOut { step: idx });
            }
        }
    }
    Ok(PreCommandOutcome::AllSucceeded)
}

/// Pump + search the session's line buffer until a line containing the
/// sentinel followed by a parseable integer appears. Returns the
/// exit code, or `None` on timeout.
///
/// Why the integer check: PTY shells typically echo the raw input line
/// back before executing it. That echo contains the sentinel verbatim
/// (e.g. `printf '__x %d\n' $?`), so a naive substring match hits the
/// echo first. The actual output line has a concrete number after the
/// sentinel — only lines matching that shape are real completion
/// signals; echoed-input lines are skipped and the wait continues.
fn wait_for_sentinel<S: TerminalServer>(
    server: &mut S,
    session_id: &SessionId,
    sentinel: &str,
    options: &PreCommandOptions,
) -> Result<Option<i32>, TerminalError> {
    let deadline = Instant::now() + options.step_timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }
        let step = remaining.min(options.pump_interval);
        server.pump(session_id, step)?;

        let hits = server.search_output(session_id, sentinel, false)?;
        if !hits.is_empty() {
            // Inspect every matching line — the first one is usually
            // the shell's input echo, the second is the real output.
            // Read a reasonable window around the hits; taking the
            // full buffer is fine at the sizes we deal with here.
            let lines = server.read_output(session_id, None, None)?;
            for idx in hits {
                if let Some(line) = lines.get(idx) {
                    if let Some(code) = parse_sentinel_exit_code(&line.content, sentinel) {
                        return Ok(Some(code));
                    }
                }
            }
            // No matching line had a parseable code yet — probably we
            // only have the input echo. Loop and pump again.
        }
    }
}

fn parse_sentinel_exit_code(line: &str, sentinel: &str) -> Option<i32> {
    // Expect "<sentinel> <code>" somewhere in the line. Trim whatever
    // prefix the shell emitted (prompt characters, bracketed paste
    // indicators) before the sentinel.
    let idx = line.find(sentinel)?;
    let tail = &line[idx + sentinel.len()..];
    tail.split_whitespace().next().and_then(|s| s.parse().ok())
}

fn transition_err(e: TransitionError) -> TerminalError {
    TerminalError::Persist(format!("pre-command FSM: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procmgr::{ManagedConfig, ManagedState};
    use crate::server::{InMemoryTerminalServer, ServerSpawnConfig};
    use crate::shell::ShellSpec;

    fn unix_only(name: &str) -> bool {
        if !cfg!(unix) {
            eprintln!("skipping {name}: unix-only");
            return false;
        }
        true
    }

    fn spawn_bash(server: &mut InMemoryTerminalServer) -> SessionId {
        server
            .create_session(ServerSpawnConfig {
                name: Some("precmd-test".into()),
                shell: Some(ShellSpec {
                    program: "/bin/bash".into(),
                    args: vec!["--norc".into(), "--noprofile".into()],
                }),
                working_dir: None,
                env: vec![],
            })
            .expect("spawn")
    }

    fn with_pre(steps: &[&str]) -> ManagedProcess {
        let mut cfg = ManagedConfig::new("slug", "name", "main-cmd");
        cfg.pre_commands = steps.iter().map(|s| (*s).to_string()).collect();
        ManagedProcess::new(cfg)
    }

    #[test]
    fn empty_pre_commands_returns_skipped_and_leaves_fsm_stopped() {
        let mut server = InMemoryTerminalServer::new();
        // Even without a real session the no-op branch fires first —
        // but creating one anyway keeps the test close to real use.
        if !unix_only("empty_pre_commands_returns_skipped_and_leaves_fsm_stopped") {
            return;
        }
        let id = spawn_bash(&mut server);
        let mut proc = with_pre(&[]);
        let out = run_pre_commands(&mut server, &id, &mut proc, &PreCommandOptions::default())
            .expect("run");
        assert_eq!(out, PreCommandOutcome::Skipped);
        assert!(out.is_success());
        assert_eq!(*proc.state(), ManagedState::Stopped);
    }

    #[test]
    fn successful_chain_advances_through_steps_and_reports_all_succeeded() {
        if !unix_only("successful_chain_advances_through_steps_and_reports_all_succeeded") {
            return;
        }
        let mut server = InMemoryTerminalServer::new();
        let id = spawn_bash(&mut server);
        let mut proc = with_pre(&["true", "echo hello > /dev/null", "true"]);
        let opts = PreCommandOptions {
            step_timeout: Duration::from_secs(5),
            pump_interval: Duration::from_millis(50),
        };
        let out = run_pre_commands(&mut server, &id, &mut proc, &opts).expect("run");
        assert_eq!(out, PreCommandOutcome::AllSucceeded);
        // FSM should be parked on the last PreCommand step — caller
        // follows up with mark_starting().
        match proc.state() {
            ManagedState::PreCommand { step } => assert_eq!(*step, 2),
            other => panic!("expected PreCommand step, got {other:?}"),
        }
    }

    #[test]
    fn failing_step_returns_step_failed_with_exit_code_and_stops_machine() {
        if !unix_only("failing_step_returns_step_failed_with_exit_code_and_stops_machine") {
            return;
        }
        let mut server = InMemoryTerminalServer::new();
        let id = spawn_bash(&mut server);
        let mut proc = with_pre(&["true", "false", "true"]);
        let opts = PreCommandOptions {
            step_timeout: Duration::from_secs(5),
            pump_interval: Duration::from_millis(50),
        };
        let out = run_pre_commands(&mut server, &id, &mut proc, &opts).expect("run");
        match out {
            PreCommandOutcome::StepFailed { step, exit_code } => {
                assert_eq!(step, 1);
                assert_eq!(exit_code, 1);
            }
            other => panic!("expected StepFailed, got {other:?}"),
        }
        assert_eq!(*proc.state(), ManagedState::Stopped);
        assert_eq!(proc.restart_attempts(), 0);
    }

    #[test]
    fn timed_out_step_returns_step_timed_out_and_stops_machine() {
        if !unix_only("timed_out_step_returns_step_timed_out_and_stops_machine") {
            return;
        }
        let mut server = InMemoryTerminalServer::new();
        let id = spawn_bash(&mut server);
        // 2-second sleep under a 200 ms step timeout — guaranteed to
        // miss the sentinel window.
        let mut proc = with_pre(&["sleep 2"]);
        let opts = PreCommandOptions {
            step_timeout: Duration::from_millis(200),
            pump_interval: Duration::from_millis(50),
        };
        let out = run_pre_commands(&mut server, &id, &mut proc, &opts).expect("run");
        assert_eq!(out, PreCommandOutcome::StepTimedOut { step: 0 });
        assert_eq!(*proc.state(), ManagedState::Stopped);
    }

    #[test]
    fn parse_sentinel_exit_code_handles_common_shell_output_shapes() {
        assert_eq!(parse_sentinel_exit_code("__x 0", "__x"), Some(0));
        assert_eq!(parse_sentinel_exit_code("prompt> __x 127", "__x"), Some(127));
        // Trailing garbage after the code is ignored.
        assert_eq!(parse_sentinel_exit_code("__x 2 extra", "__x"), Some(2));
        // No sentinel → None.
        assert_eq!(parse_sentinel_exit_code("boring", "__x"), None);
        // Sentinel without a code → fall through to None so caller
        // treats it as "no parseable result"; the runner policy
        // (treat-as-success) lives one level up.
        assert_eq!(parse_sentinel_exit_code("__x nothing-after", "__x"), None);
    }

    #[test]
    fn outcome_is_success_covers_all_passing_variants() {
        assert!(PreCommandOutcome::AllSucceeded.is_success());
        assert!(PreCommandOutcome::Skipped.is_success());
        assert!(
            !PreCommandOutcome::StepFailed { step: 0, exit_code: 1 }.is_success(),
        );
        assert!(!PreCommandOutcome::StepTimedOut { step: 0 }.is_success());
    }
}
