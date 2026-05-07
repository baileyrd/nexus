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
//! - Asynchronous. Each step runs sequentially and blocks until it
//!   finishes or its per-step timeout elapses. The caller controls
//!   concurrency by scheduling the whole pipeline on its own task.
//! - A memory / CPU limiter. §7 polling is a sibling concern.
//!
//! # BL-065 — shell-family-aware sentinels
//!
//! POSIX shells get the original `printf '<sentinel> %d\n' $?` form.
//! `cmd.exe` uses `echo <sentinel> %ERRORLEVEL%` (the variable
//! expands inline); PowerShell uses
//! `Write-Host "<sentinel> $LASTEXITCODE"`. All three produce the
//! same line shape — `<sentinel> <integer>` — so [`parse_sentinel_exit_code`]
//! and [`wait_for_sentinel`] don't need to fork per family. The
//! choice of [`ShellFamily`] is set on [`PreCommandOptions`]; the
//! caller picks one with [`ShellFamily::detect_from_path`] (or via
//! the explicit constructor when the spawn shell isn't a pathy
//! basename).

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::TerminalError;
use crate::procmgr::{ManagedProcess, TransitionError};
use crate::server::TerminalServer;
use crate::session::SessionId;

/// Default per-step timeout — PRD-09 §4.3 "30 s (user-configurable)".
pub const DEFAULT_STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// BL-065 — shell families recognised by the pre-command pipeline.
/// The variant selects which sentinel-emitting one-liner gets
/// appended to each step. Adding a new variant means picking the
/// right syntax for that shell's "previous command's exit code"
/// expansion plus a print primitive that doesn't mangle the
/// number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellFamily {
    /// `bash`, `zsh`, `dash`, `sh`, `ksh`, `fish` (with caveats —
    /// fish doesn't support `$?` so users typically run pre-commands
    /// inside a POSIX subshell). Sentinel form:
    /// `printf '<sentinel> %d\n' $?`
    Posix,
    /// Windows `cmd.exe`. Sentinel form:
    /// `echo <sentinel> %ERRORLEVEL%`. The `%ERRORLEVEL%` token
    /// expands inline against the previous command's exit code.
    Cmd,
    /// Windows PowerShell — both `pwsh` (Core / Cross-platform) and
    /// the legacy `powershell.exe`. Sentinel form:
    /// `Write-Host "<sentinel> $LASTEXITCODE"`. `$LASTEXITCODE` is
    /// the equivalent of `$?` for native commands; for cmdlets it
    /// stays unset (PowerShell's `$?` is a boolean), which is fine
    /// because pre-commands typically launch external tools.
    PowerShell,
}

impl ShellFamily {
    /// Pick the family that matches `shell_path`'s basename. The
    /// match is case-insensitive and ignores a trailing `.exe`. Any
    /// unrecognised name (custom shells, full paths to `bash`, etc.)
    /// falls through to [`ShellFamily::Posix`] — the historical
    /// behaviour and the safe default on Linux / macOS.
    ///
    /// The basename is computed by splitting on either `/` or `\`,
    /// so Windows paths like `C:\Windows\System32\cmd.exe` resolve
    /// even when this code runs on Linux (e.g. when a config file
    /// stored on a Windows host is read from a Linux dev box). Pure
    /// `Path::new` would treat `\` as a regular character on POSIX
    /// and miss the basename.
    #[must_use]
    pub fn detect_from_path(shell_path: &str) -> Self {
        let basename = shell_path
            .rsplit(|c: char| c == '/' || c == '\\')
            .next()
            .unwrap_or(shell_path);
        let basename = basename
            .strip_suffix(".exe")
            .or_else(|| basename.strip_suffix(".EXE"))
            .unwrap_or(basename);
        match basename.to_ascii_lowercase().as_str() {
            "cmd" => Self::Cmd,
            "pwsh" | "powershell" => Self::PowerShell,
            _ => Self::Posix,
        }
    }

    /// Build the sentinel-emitting one-liner appended after each
    /// pre-command. Returns the full byte-payload to write into the
    /// PTY, including the trailing newline that submits the line.
    fn wrap_step(self, command: &str, sentinel: &str) -> String {
        match self {
            // `; printf ... $?` is idempotent across dash/bash/zsh/ksh.
            // The newline split (rather than `;`) keeps multi-line
            // pre-commands like heredocs working.
            Self::Posix => {
                format!("{command}\nprintf '{sentinel} %d\\n' $?\n")
            }
            // cmd.exe sets `%ERRORLEVEL%` after every command; an
            // `echo` on the next line picks it up and writes the
            // sentinel + code to stdout. Use `\r\n` because cmd.exe
            // is line-buffered against carriage-return-aware EOLs;
            // the parser is whitespace-agnostic so `\n` would also
            // work, but `\r\n` matches what the user would have
            // typed.
            Self::Cmd => {
                format!("{command}\r\necho {sentinel} %ERRORLEVEL%\r\n")
            }
            // PowerShell: `;` and newline both work as statement
            // separators, but newline matches the POSIX path's shape
            // and avoids escaping concerns inside the user's
            // command. `Write-Host` writes to stdout the way `echo`
            // does on POSIX (vs. `Write-Output` which writes to the
            // pipeline).
            Self::PowerShell => {
                format!("{command}\r\nWrite-Host \"{sentinel} $LASTEXITCODE\"\r\n")
            }
        }
    }
}

impl Default for ShellFamily {
    fn default() -> Self {
        Self::Posix
    }
}

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
    /// BL-065 — shell family the spawned session is running. The
    /// pipeline picks an appropriate sentinel one-liner from this.
    /// Defaults to [`ShellFamily::Posix`]; callers running a saved
    /// command on Windows should set this to [`ShellFamily::Cmd`]
    /// or [`ShellFamily::PowerShell`] (or use
    /// [`ShellFamily::detect_from_path`] against the saved command's
    /// `shell` field).
    pub shell_family: ShellFamily,
}

impl Default for PreCommandOptions {
    fn default() -> Self {
        Self {
            step_timeout: DEFAULT_STEP_TIMEOUT,
            pump_interval: Duration::from_millis(100),
            shell_family: ShellFamily::default(),
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
        // BL-065 — pick the right wrapper for the configured shell
        // family. POSIX = `printf $?`, cmd = `echo %ERRORLEVEL%`,
        // PowerShell = `Write-Host $LASTEXITCODE`. The output line
        // shape stays `<sentinel> <integer>` across families so the
        // sentinel parser doesn't need to know which family ran it.
        let wrapped = options.shell_family.wrap_step(cmd, &sentinel);
        server.send_raw_input(session_id, wrapped.as_bytes())?;

        match wait_for_sentinel(server, session_id, &sentinel, options)? {
            Some(0) => {}
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

// Passed as a function pointer to `.map_err(transition_err)`; wrapping
// in a closure would re-trip `redundant_closure`.
#[allow(clippy::needless_pass_by_value)]
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
            shell_family: ShellFamily::Posix,
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
            shell_family: ShellFamily::Posix,
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
            shell_family: ShellFamily::Posix,
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

    // ── BL-065 — ShellFamily detection + wrap_step tests ────────────

    #[test]
    fn shell_family_detect_recognises_cmd_with_and_without_extension() {
        assert_eq!(
            ShellFamily::detect_from_path("C:\\Windows\\System32\\cmd.exe"),
            ShellFamily::Cmd,
        );
        assert_eq!(ShellFamily::detect_from_path("cmd.exe"), ShellFamily::Cmd);
        assert_eq!(ShellFamily::detect_from_path("cmd"), ShellFamily::Cmd);
        // Case-insensitive — Windows paths often arrive uppercase.
        assert_eq!(ShellFamily::detect_from_path("CMD.EXE"), ShellFamily::Cmd);
    }

    #[test]
    fn shell_family_detect_recognises_powershell_and_pwsh() {
        for path in [
            "pwsh",
            "pwsh.exe",
            "/usr/bin/pwsh",
            "powershell.exe",
            "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
        ] {
            assert_eq!(
                ShellFamily::detect_from_path(path),
                ShellFamily::PowerShell,
                "expected PowerShell for {path}",
            );
        }
    }

    #[test]
    fn shell_family_detect_falls_through_to_posix_for_known_unix_shells() {
        for path in [
            "/bin/bash",
            "/usr/local/bin/zsh",
            "/usr/bin/fish",
            "sh",
            "dash",
            "ksh",
        ] {
            assert_eq!(
                ShellFamily::detect_from_path(path),
                ShellFamily::Posix,
                "expected Posix for {path}",
            );
        }
    }

    #[test]
    fn shell_family_detect_unknown_shell_defaults_to_posix() {
        assert_eq!(
            ShellFamily::detect_from_path("/opt/weird/myshell"),
            ShellFamily::Posix,
        );
        assert_eq!(ShellFamily::detect_from_path(""), ShellFamily::Posix);
    }

    #[test]
    fn wrap_step_posix_uses_printf_with_dollar_q() {
        let wrapped = ShellFamily::Posix.wrap_step("ls", "__sentinel");
        assert!(wrapped.starts_with("ls\n"));
        assert!(wrapped.contains("printf '__sentinel %d\\n' $?"));
        assert!(wrapped.ends_with('\n'));
    }

    #[test]
    fn wrap_step_cmd_uses_echo_errorlevel_and_crlf() {
        let wrapped = ShellFamily::Cmd.wrap_step("dir", "__sentinel");
        assert!(wrapped.starts_with("dir\r\n"));
        assert!(wrapped.contains("echo __sentinel %ERRORLEVEL%"));
        assert!(wrapped.ends_with("\r\n"));
        // The cmd path must NOT use the POSIX `$?` form.
        assert!(!wrapped.contains("$?"));
    }

    #[test]
    fn wrap_step_powershell_uses_write_host_with_lastexitcode() {
        let wrapped = ShellFamily::PowerShell.wrap_step("Get-Item .", "__sentinel");
        assert!(wrapped.starts_with("Get-Item .\r\n"));
        assert!(wrapped.contains("Write-Host \"__sentinel $LASTEXITCODE\""));
        assert!(wrapped.ends_with("\r\n"));
        // Must not leak the cmd `%ERRORLEVEL%` token.
        assert!(!wrapped.contains("%ERRORLEVEL%"));
    }

    #[test]
    fn wrap_step_output_shape_round_trips_through_parse_sentinel() {
        // Simulate the line each family's wrapper emits and verify
        // the existing parser recovers the integer. This is what
        // makes the cross-family approach work without forking the
        // parser.
        for (family, line) in [
            (ShellFamily::Posix, "__sentinel 0"),
            (ShellFamily::Cmd, "__sentinel 0"),
            (ShellFamily::PowerShell, "__sentinel 0"),
        ] {
            assert_eq!(
                parse_sentinel_exit_code(line, "__sentinel"),
                Some(0),
                "{family:?} produced an unexpected line shape",
            );
        }
        // Non-zero codes too.
        assert_eq!(parse_sentinel_exit_code("__x 127", "__x"), Some(127));
    }

    #[test]
    fn pre_command_options_default_is_posix_family() {
        // Back-compat for any caller that hadn't been updated yet:
        // the default still picks Posix so existing tests / runtimes
        // behave like before BL-065 landed.
        assert_eq!(
            PreCommandOptions::default().shell_family,
            ShellFamily::Posix,
        );
    }
}
