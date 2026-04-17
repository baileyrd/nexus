//! Process-manager state machine (PRD-09 §4).
//!
//! # Scope
//!
//! The session-level [`crate::ProcessState`] tracks the **PTY child's**
//! lifecycle: Running / Exited / Killed. That's not enough for the
//! process-manager sidebar, which needs to reason about pre-commands,
//! startup handshakes, auto-restart, and backoff across *multiple*
//! session spawns for the same saved command. This module layers those
//! richer states on top — it is driven by an external scheduler (the
//! UI or a core plugin), not by [`crate::Session`].
//!
//! # Microkernel fit
//!
//! Pure library. No kernel IPC, no plugin boundary, no spawning. The
//! caller advances the state machine by invoking the explicit
//! transition methods; this crate only validates which transitions are
//! legal and computes the backoff delay for restarts.
//!
//! # What this is NOT
//!
//! - A task scheduler. Pre-commands, startup probes, restart waits, and
//!   memory polling (§4.3, §7.2) are wall-clock concerns that belong to
//!   the UI / core-plugin driver. This module tracks *which step is
//!   current* and *whether a transition is valid* — not *when* to fire
//!   the next step.
//! - A replacement for [`crate::ProcessState`]. The two coexist: the
//!   managed machine wraps the session machine, and [`ManagedState`]
//!   maps to a session state only while the child is actually running.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Per-step timeout default — PRD-09 §4.3 "30s user-configurable".
pub const DEFAULT_PRE_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// Auto-restart schedule (PRD-09 §4.3). The delay for attempt `n`
/// (1-indexed) is `backoffs[min(n-1, len-1)]`.
pub const DEFAULT_AUTO_RESTART_BACKOFF_MS: [u64; 3] = [2_000, 5_000, 10_000];

/// User-configured wrapper for a managed process (PRD-09 §13 schema).
/// Names mirror the `procmgr_commands` columns so the core-plugin layer
/// can round-trip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedConfig {
    /// URL-safe identifier.
    pub slug: String,
    /// Human-readable label.
    pub name: String,
    /// Main shell command (may include `&&` / `||` — see
    /// [`crate::compound`]).
    pub shell_cmd: String,
    /// Ordered pre-commands to run before the main command (PRD-09 §4.4).
    pub pre_commands: Vec<String>,
    /// Whether [`ManagedState::Crashed`] should transition back into
    /// [`ManagedState::Restarting`] automatically.
    pub auto_restart: bool,
    /// Backoff schedule for restart attempts.
    pub auto_restart_backoff_ms: Vec<u64>,
    /// Cap on restart attempts before giving up and parking in
    /// [`ManagedState::Stopped`] (PRD-09 §4.3, default 10).
    pub max_restart_attempts: u32,
}

impl ManagedConfig {
    /// Build a baseline config — no pre-commands, no auto-restart,
    /// standard backoff.
    #[must_use]
    pub fn new(slug: impl Into<String>, name: impl Into<String>, shell_cmd: impl Into<String>) -> Self {
        Self {
            slug: slug.into(),
            name: name.into(),
            shell_cmd: shell_cmd.into(),
            pre_commands: Vec::new(),
            auto_restart: false,
            auto_restart_backoff_ms: DEFAULT_AUTO_RESTART_BACKOFF_MS.to_vec(),
            max_restart_attempts: 10,
        }
    }

    /// Delay for restart attempt `attempt` (1-indexed). Returns 0 ms if
    /// backoff table is empty.
    #[must_use]
    pub fn backoff_for(&self, attempt: u32) -> Duration {
        if self.auto_restart_backoff_ms.is_empty() {
            return Duration::ZERO;
        }
        let idx = (attempt.saturating_sub(1) as usize).min(self.auto_restart_backoff_ms.len() - 1);
        Duration::from_millis(self.auto_restart_backoff_ms[idx])
    }
}

impl Default for ManagedConfig {
    fn default() -> Self {
        Self::new("default", "default", "")
    }
}

/// PRD-09 §4.1 state diagram, encoded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ManagedState {
    /// No process running. Fresh configs, clean exits, and give-up
    /// states all land here.
    Stopped,
    /// Executing one of the pre-commands; `step` is 0-indexed.
    PreCommand {
        /// Which pre-command is currently running.
        step: usize,
    },
    /// Main command spawned, awaiting first output / handshake.
    Starting,
    /// Main command is live and producing output.
    Running,
    /// Abnormal exit. Will transition to
    /// [`ManagedState::Restarting`] if `auto_restart` is enabled and
    /// attempts are still available, otherwise back to
    /// [`ManagedState::Stopped`].
    Crashed {
        /// Exit code of the crashed child (if observed).
        exit_code: Option<i32>,
    },
    /// Waiting for the backoff delay before re-spawning.
    Restarting {
        /// 1-indexed count of restart attempts so far.
        attempt: u32,
        /// Delay (ms) the driver should wait before calling
        /// [`ManagedProcess::mark_starting`] again.
        delay_ms: u64,
    },
}

/// A running (or idle) managed process.
#[derive(Debug, Clone)]
pub struct ManagedProcess {
    config: ManagedConfig,
    state: ManagedState,
    /// Number of restart attempts used in the current crash chain.
    /// Reset on [`Self::mark_running`] so a healthy process that later
    /// crashes starts the backoff schedule from `attempt=1` again.
    restart_attempts: u32,
}

/// Why a transition was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    /// The call is not legal in the current state.
    #[error("transition not allowed: {from:?} → {to}")]
    Illegal {
        /// State the process was in.
        from: ManagedState,
        /// Attempted transition label.
        to: &'static str,
    },
    /// Pre-command step index overran the configured list length.
    #[error("pre-command step {step} out of range (have {total})")]
    PreCommandOutOfRange {
        /// Requested step index.
        step: usize,
        /// Number of configured pre-commands.
        total: usize,
    },
    /// Auto-restart cap reached (PRD-09 §4.3). The process is parked in
    /// [`ManagedState::Stopped`] after this.
    #[error("max restart attempts reached ({attempts})")]
    RestartExhausted {
        /// How many attempts have been used.
        attempts: u32,
    },
}

impl ManagedProcess {
    /// Create a managed process in [`ManagedState::Stopped`].
    #[must_use]
    pub fn new(config: ManagedConfig) -> Self {
        Self {
            config,
            state: ManagedState::Stopped,
            restart_attempts: 0,
        }
    }

    /// Current state.
    #[must_use]
    pub fn state(&self) -> &ManagedState {
        &self.state
    }

    /// Immutable access to the underlying config.
    #[must_use]
    pub fn config(&self) -> &ManagedConfig {
        &self.config
    }

    /// Number of restart attempts used in the current crash chain.
    #[must_use]
    pub fn restart_attempts(&self) -> u32 {
        self.restart_attempts
    }

    /// Reset state to [`ManagedState::Stopped`] and clear the restart
    /// counter. Used when the user manually stops the process.
    pub fn mark_stopped(&mut self) {
        self.state = ManagedState::Stopped;
        self.restart_attempts = 0;
    }

    /// Enter [`ManagedState::PreCommand`] for `step`. Legal only from
    /// Stopped (starting the chain) or from a prior PreCommand step
    /// that has just completed (advancing the chain).
    ///
    /// # Errors
    /// - [`TransitionError::Illegal`] if called from Running / Starting /
    ///   Crashed / Restarting.
    /// - [`TransitionError::PreCommandOutOfRange`] if `step` >=
    ///   `config.pre_commands.len()`.
    pub fn mark_pre_command(&mut self, step: usize) -> Result<(), TransitionError> {
        if step >= self.config.pre_commands.len() {
            return Err(TransitionError::PreCommandOutOfRange {
                step,
                total: self.config.pre_commands.len(),
            });
        }
        match self.state {
            ManagedState::Stopped | ManagedState::PreCommand { .. } => {
                self.state = ManagedState::PreCommand { step };
                Ok(())
            }
            _ => Err(TransitionError::Illegal {
                from: self.state.clone(),
                to: "PreCommand",
            }),
        }
    }

    /// Transition into [`ManagedState::Starting`]. Legal from Stopped
    /// (no pre-commands configured), the last PreCommand step (pre-chain
    /// finished), or Restarting (backoff elapsed).
    ///
    /// # Errors
    /// [`TransitionError::Illegal`] if called from Running / Crashed or
    /// from a non-final PreCommand step.
    pub fn mark_starting(&mut self) -> Result<(), TransitionError> {
        let legal = match &self.state {
            ManagedState::Stopped => self.config.pre_commands.is_empty(),
            ManagedState::PreCommand { step } => *step + 1 == self.config.pre_commands.len(),
            ManagedState::Restarting { .. } => true,
            _ => false,
        };
        if !legal {
            return Err(TransitionError::Illegal {
                from: self.state.clone(),
                to: "Starting",
            });
        }
        self.state = ManagedState::Starting;
        Ok(())
    }

    /// Transition from [`ManagedState::Starting`] to
    /// [`ManagedState::Running`]. Resets the restart-attempt counter —
    /// a process that successfully re-enters Running "earns back" its
    /// backoff budget.
    ///
    /// # Errors
    /// [`TransitionError::Illegal`] if called from any state other than
    /// Starting.
    pub fn mark_running(&mut self) -> Result<(), TransitionError> {
        if !matches!(self.state, ManagedState::Starting) {
            return Err(TransitionError::Illegal {
                from: self.state.clone(),
                to: "Running",
            });
        }
        self.state = ManagedState::Running;
        self.restart_attempts = 0;
        Ok(())
    }

    /// Transition from [`ManagedState::Starting`] or
    /// [`ManagedState::Running`] into [`ManagedState::Crashed`].
    /// Returns whether the caller should follow up with
    /// [`Self::mark_restarting`]: `true` iff auto-restart is enabled
    /// and [`Self::restart_attempts`] is still below
    /// `config.max_restart_attempts`.
    ///
    /// # Errors
    /// [`TransitionError::Illegal`] from Stopped / Crashed / PreCommand /
    /// Restarting.
    pub fn mark_crashed(&mut self, exit_code: Option<i32>) -> Result<bool, TransitionError> {
        if !matches!(self.state, ManagedState::Starting | ManagedState::Running) {
            return Err(TransitionError::Illegal {
                from: self.state.clone(),
                to: "Crashed",
            });
        }
        self.state = ManagedState::Crashed { exit_code };
        let can_retry = self.config.auto_restart
            && self.restart_attempts < self.config.max_restart_attempts;
        Ok(can_retry)
    }

    /// Transition from [`ManagedState::Crashed`] into
    /// [`ManagedState::Restarting`], picking the backoff delay from
    /// the config. Bumps [`Self::restart_attempts`] by one.
    ///
    /// # Errors
    /// - [`TransitionError::Illegal`] from any state other than Crashed.
    /// - [`TransitionError::RestartExhausted`] if
    ///   [`Self::restart_attempts`] is already at
    ///   `config.max_restart_attempts`; state drops to Stopped so the
    ///   driver sees a clean parked machine.
    pub fn mark_restarting(&mut self) -> Result<Duration, TransitionError> {
        if !matches!(self.state, ManagedState::Crashed { .. }) {
            return Err(TransitionError::Illegal {
                from: self.state.clone(),
                to: "Restarting",
            });
        }
        if self.restart_attempts >= self.config.max_restart_attempts {
            self.state = ManagedState::Stopped;
            return Err(TransitionError::RestartExhausted {
                attempts: self.restart_attempts,
            });
        }
        self.restart_attempts += 1;
        let delay = self.config.backoff_for(self.restart_attempts);
        self.state = ManagedState::Restarting {
            attempt: self.restart_attempts,
            delay_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
        };
        Ok(delay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with_pre(pre: &[&str]) -> ManagedConfig {
        let mut cfg = ManagedConfig::new("slug", "name", "run-me");
        cfg.pre_commands = pre.iter().map(|s| (*s).to_string()).collect();
        cfg
    }

    #[test]
    fn backoff_uses_last_entry_once_schedule_is_exhausted() {
        let mut cfg = ManagedConfig::new("s", "n", "c");
        cfg.auto_restart_backoff_ms = vec![1_000, 2_000, 5_000];
        assert_eq!(cfg.backoff_for(1), Duration::from_millis(1_000));
        assert_eq!(cfg.backoff_for(3), Duration::from_millis(5_000));
        // 4th attempt clamps to the last entry.
        assert_eq!(cfg.backoff_for(4), Duration::from_millis(5_000));
        assert_eq!(cfg.backoff_for(10), Duration::from_millis(5_000));
    }

    #[test]
    fn backoff_of_empty_schedule_is_zero() {
        let mut cfg = ManagedConfig::new("s", "n", "c");
        cfg.auto_restart_backoff_ms = vec![];
        assert_eq!(cfg.backoff_for(1), Duration::ZERO);
    }

    #[test]
    fn happy_path_with_no_pre_commands() {
        let mut p = ManagedProcess::new(ManagedConfig::new("s", "n", "c"));
        assert_eq!(*p.state(), ManagedState::Stopped);
        p.mark_starting().expect("start");
        assert_eq!(*p.state(), ManagedState::Starting);
        p.mark_running().expect("run");
        assert_eq!(*p.state(), ManagedState::Running);
    }

    #[test]
    fn happy_path_with_pre_commands() {
        let mut p = ManagedProcess::new(cfg_with_pre(&["a", "b"]));
        p.mark_pre_command(0).expect("step 0");
        assert_eq!(*p.state(), ManagedState::PreCommand { step: 0 });
        p.mark_pre_command(1).expect("step 1");
        assert_eq!(*p.state(), ManagedState::PreCommand { step: 1 });
        p.mark_starting().expect("start");
        p.mark_running().expect("run");
    }

    #[test]
    fn mark_starting_refuses_from_mid_chain_pre_command() {
        let mut p = ManagedProcess::new(cfg_with_pre(&["a", "b"]));
        p.mark_pre_command(0).expect("step 0");
        let err = p.mark_starting().unwrap_err();
        assert!(matches!(err, TransitionError::Illegal { .. }));
    }

    #[test]
    fn mark_starting_refuses_from_stopped_when_pre_commands_configured() {
        let mut p = ManagedProcess::new(cfg_with_pre(&["a"]));
        let err = p.mark_starting().unwrap_err();
        assert!(matches!(err, TransitionError::Illegal { .. }));
    }

    #[test]
    fn pre_command_out_of_range_rejected() {
        let mut p = ManagedProcess::new(cfg_with_pre(&["a"]));
        let err = p.mark_pre_command(1).unwrap_err();
        assert!(matches!(err, TransitionError::PreCommandOutOfRange { .. }));
    }

    #[test]
    fn crash_without_auto_restart_returns_false_and_parks_in_crashed() {
        let mut p = ManagedProcess::new(ManagedConfig::new("s", "n", "c"));
        p.mark_starting().expect("start");
        p.mark_running().expect("run");
        let can_retry = p.mark_crashed(Some(1)).expect("crash");
        assert!(!can_retry);
        assert!(matches!(p.state(), ManagedState::Crashed { exit_code: Some(1) }));
    }

    #[test]
    fn crash_then_restart_then_run_resets_attempt_counter() {
        let mut cfg = ManagedConfig::new("s", "n", "c");
        cfg.auto_restart = true;
        cfg.auto_restart_backoff_ms = vec![100, 200];
        cfg.max_restart_attempts = 5;
        let mut p = ManagedProcess::new(cfg);

        p.mark_starting().expect("start");
        p.mark_running().expect("run");
        assert!(p.mark_crashed(Some(1)).expect("crash 1"));
        let d1 = p.mark_restarting().expect("restart 1");
        assert_eq!(d1, Duration::from_millis(100));
        assert_eq!(p.restart_attempts(), 1);

        p.mark_starting().expect("re-start");
        p.mark_running().expect("re-run"); // resets counter
        assert_eq!(p.restart_attempts(), 0);

        // A later crash starts the backoff schedule over at step 1.
        assert!(p.mark_crashed(Some(1)).expect("crash 2"));
        let d2 = p.mark_restarting().expect("restart 2");
        assert_eq!(d2, Duration::from_millis(100));
    }

    #[test]
    fn restart_exhaustion_parks_machine_in_stopped() {
        let mut cfg = ManagedConfig::new("s", "n", "c");
        cfg.auto_restart = true;
        cfg.auto_restart_backoff_ms = vec![1];
        cfg.max_restart_attempts = 2;
        let mut p = ManagedProcess::new(cfg);

        p.mark_starting().expect("start");
        p.mark_running().expect("run");

        // Attempt 1
        assert!(p.mark_crashed(Some(1)).expect("crash"));
        p.mark_restarting().expect("restart 1");
        p.mark_starting().expect("start 1");
        // Attempt 2 — crash during Starting, before we reach Running
        assert!(p.mark_crashed(None).expect("crash 2"));
        p.mark_restarting().expect("restart 2");
        p.mark_starting().expect("start 2");
        // Attempt 3 would exceed max — caller gets RestartExhausted and
        // the machine drops to Stopped so UI sees a clean park.
        assert!(!p.mark_crashed(None).expect("crash 3"));
        let err = p.mark_restarting().unwrap_err();
        assert!(matches!(err, TransitionError::RestartExhausted { attempts: 2 }));
        assert_eq!(*p.state(), ManagedState::Stopped);
    }

    #[test]
    fn mark_running_illegal_from_stopped() {
        let mut p = ManagedProcess::new(ManagedConfig::new("s", "n", "c"));
        let err = p.mark_running().unwrap_err();
        assert!(matches!(err, TransitionError::Illegal { .. }));
    }

    #[test]
    fn mark_stopped_resets_restart_counter() {
        let mut cfg = ManagedConfig::new("s", "n", "c");
        cfg.auto_restart = true;
        cfg.auto_restart_backoff_ms = vec![1];
        cfg.max_restart_attempts = 5;
        let mut p = ManagedProcess::new(cfg);
        p.mark_starting().expect("start");
        p.mark_running().expect("run");
        p.mark_crashed(None).expect("crash");
        p.mark_restarting().expect("restart");
        assert_eq!(p.restart_attempts(), 1);
        p.mark_stopped();
        assert_eq!(p.restart_attempts(), 0);
        assert_eq!(*p.state(), ManagedState::Stopped);
    }
}
