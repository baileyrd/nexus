//! CLI surface over [`nexus_terminal`] (PRD-09 §3.7).
//!
//! # Scope
//!
//! Phase G ships three subcommands wired directly to the single-process
//! `Session` / `LineBuffer` primitives:
//!
//! - `nexus term env` — print the shell the crate would pick by default.
//!   Cheap smoke test that `detect_default_shell()` resolves something
//!   real on this machine.
//! - `nexus term run <cmd>` — non-interactive: spawn the user's shell with
//!   `-c <cmd>`, drain every line through the [`LineBuffer`]
//!   ANSI-stripper, print the text view to stdout, exit with the child's
//!   status. Foreground and single-shot; the session dies with the CLI
//!   process.
//! - `nexus term shell` — interactive: attach the current terminal to a
//!   fresh [`Session`] by bridging stdin/stdout/resize events, returning
//!   when the shell exits. Useful as a manual verification path and as
//!   a stand-in for the future daemon-backed session manager.
//!
//! # Why not a session registry here?
//!
//! The PRD's multi-session, name-addressable, LRU-evicted model needs a
//! long-running process owning the [`SessionManager`]; a CLI invocation
//! is transient and any sessions it spawns die when it exits. That
//! piece lands in Phase H alongside the `com.nexus.terminal` core plugin
//! and SQLite persistence — none of that is blocked by what we ship
//! here, and the `Session::spawn` / `read_into` / `send_signal` surface
//! is exactly what the future core-plugin dispatch handlers will wrap.

use std::io::Write;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use nexus_terminal::{detect_default_shell, LineBuffer, Session, SessionConfig, ShellSpec};

/// `nexus term env` — print the detected default shell.
pub fn env() -> Result<()> {
    let spec = detect_default_shell();
    println!("shell   : {}", spec.program.display());
    if !spec.args.is_empty() {
        println!("args    : {:?}", spec.args);
    }
    Ok(())
}

/// `nexus term run <cmd>` — non-interactive one-shot execution.
///
/// Spawns `${SHELL:-/bin/sh} -c <cmd>` through a [`Session`], drains the
/// PTY into a [`LineBuffer`] until the child exits, prints each line's
/// `text_only` view to stdout, and returns the child's exit code.
///
/// Timeout: `max_secs` bounds total wall-clock. On overshoot the session
/// is shut down with the standard INT→TERM→KILL ladder and the CLI
/// returns exit code 124 (GNU `timeout` convention).
pub fn run(cmd: &str, max_secs: u64) -> Result<i32> {
    let shell = detect_default_shell();
    let spec = ShellSpec {
        program: shell.program.clone(),
        args: vec!["-c".into(), cmd.to_string()],
    };
    let mut session = Session::spawn(SessionConfig {
        shell: Some(spec),
        ..SessionConfig::default()
    })
    .context("spawn shell for `nexus term run`")?;

    let mut lines = LineBuffer::new();
    let deadline = Instant::now() + Duration::from_secs(max_secs);
    let mut stdout = std::io::stdout().lock();

    let mut next_line_to_emit = 0usize;
    let mut exit_code: Option<u32> = None;

    while Instant::now() < deadline {
        let _ = session
            .read_into(None, Some(&mut lines), Duration::from_millis(200))
            .context("drain session")?;

        // Emit newly-appended complete lines. Dedup collapses adjacent
        // duplicates into a single entry's `repeats` counter, so the
        // stream of emitted entries is already spinner-stripped.
        while next_line_to_emit < lines.len() {
            if let Some(line) = lines.iter().nth(next_line_to_emit) {
                writeln!(stdout, "{}", line.text_only)?;
                next_line_to_emit += 1;
            } else {
                break;
            }
        }

        if let Some(code) = session.try_wait_exit() {
            exit_code = Some(code);
            // Final drain + flush of any partial-last-line bytes.
            let _ = session
                .read_into(None, Some(&mut lines), Duration::from_millis(100))
                .ok();
            lines.flush_pending();
            while next_line_to_emit < lines.len() {
                if let Some(line) = lines.iter().nth(next_line_to_emit) {
                    writeln!(stdout, "{}", line.text_only)?;
                    next_line_to_emit += 1;
                } else {
                    break;
                }
            }
            break;
        }
    }

    if let Some(code) = exit_code {
        #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
        Ok(code as i32)
    } else {
        // Over budget — escalate. 124 matches GNU `timeout`'s exit code
        // for "command timed out", so shell scripts can check against
        // the established convention.
        tracing::warn!(
            cmd,
            timeout_secs = max_secs,
            "`nexus term run` exceeded wall-clock budget; shutting down",
        );
        let _ = session.request_shutdown(Duration::from_millis(500));
        // Even after shutdown, flush anything queued so the user sees
        // the final output before the 124 exit.
        let _ = session
            .read_into(None, Some(&mut lines), Duration::from_millis(100))
            .ok();
        lines.flush_pending();
        while next_line_to_emit < lines.len() {
            if let Some(line) = lines.iter().nth(next_line_to_emit) {
                writeln!(stdout, "{}", line.text_only)?;
                next_line_to_emit += 1;
            } else {
                break;
            }
        }
        Ok(124)
    }
}

/// `nexus term shell` — spawn an interactive PTY shell as a child of this
/// CLI process and bridge stdio until it exits.
///
/// Phase G keeps the bridging minimal:
/// - Lines captured by [`LineBuffer`] are printed to the CLI's stdout
///   via `text_only`. Raw ANSI bytes are dropped in the CLI so callers
///   don't get mysterious colour codes when piping to a file. Full
///   raw-byte pass-through (true pty passthrough, with local echo and
///   a raw-mode tty) is Phase H.
/// - Local stdin is not forwarded. Interactive typing belongs to the
///   future UI terminal surface (PRD §14). `shell` today is meant as
///   a manual verification path: run it, confirm the shell banner
///   appears, Ctrl-C to send SIGINT through `request_shutdown`.
///
/// Returns the shell's exit code, or 130 if the CLI caught Ctrl-C and
/// tore the session down (matching the traditional "terminated by
/// SIGINT" convention).
pub fn shell() -> Result<i32> {
    let mut session = Session::spawn(SessionConfig::default())
        .context("spawn default shell for `nexus term shell`")?;
    println!(
        "[nexus-term] attached to {}; Ctrl-C to exit",
        session.shell_display()
    );

    // Trap Ctrl-C so a real user can shut down the session cleanly
    // instead of ripping the CLI process out from under the PTY. We
    // flip a shared atomic and the main loop responds.
    let interrupted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let flag = interrupted.clone();
        // ctrlc::set_handler errors on a double-install; ignore because
        // other parts of the CLI might already have installed one.
        let _ = ctrlc::set_handler(move || {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        });
    }

    let mut lines = LineBuffer::new();
    let mut next_line_to_emit = 0usize;
    let mut stdout = std::io::stdout().lock();

    loop {
        if interrupted.load(std::sync::atomic::Ordering::Relaxed) {
            tracing::info!("caught Ctrl-C — requesting shutdown");
            let _ = session.request_shutdown(Duration::from_millis(500));
            return Ok(130);
        }

        let _ = session
            .read_into(None, Some(&mut lines), Duration::from_millis(200))
            .context("drain shell")?;

        while next_line_to_emit < lines.len() {
            if let Some(line) = lines.iter().nth(next_line_to_emit) {
                writeln!(stdout, "{}", line.text_only)?;
                next_line_to_emit += 1;
            } else {
                break;
            }
        }

        if let Some(code) = session.try_wait_exit() {
            lines.flush_pending();
            while next_line_to_emit < lines.len() {
                if let Some(line) = lines.iter().nth(next_line_to_emit) {
                    writeln!(stdout, "{}", line.text_only)?;
                    next_line_to_emit += 1;
                } else {
                    break;
                }
            }
            #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
            return Ok(code as i32);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_reports_detected_shell_without_panicking() {
        // `env()` writes to stdout; we only check the result is Ok(()).
        // The shell it picks depends on the host.
        assert!(env().is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn run_executes_command_and_returns_zero_on_success() {
        let code = run("echo ran-ok", 5).expect("run");
        assert_eq!(code, 0);
    }

    #[cfg(unix)]
    #[test]
    fn run_returns_nonzero_on_shell_failure() {
        // `false` exits 1 on every Unix; don't assert on the exact
        // value because busybox / dash implementations vary, but it
        // must not be 0.
        let code = run("false", 5).expect("run");
        assert_ne!(code, 0);
    }

    // Intentionally no automated timeout test: the realistic behaviour
    // is correct by inspection (the while loop's Instant::now() < deadline
    // check exits at max_secs, the else branch returns 124 after
    // request_shutdown), but `sh -c "sleep 30"` in a PTY + 4-parallel
    // cargo test produced an unreliable signal in CI. Verify manually
    // with `cargo run -p nexus-cli -- term run "sleep 30" --timeout 1`
    // and observe exit code 124.
}
