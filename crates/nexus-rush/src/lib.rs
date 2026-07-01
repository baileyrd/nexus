//! nexus-rush — a small Rust shell, vendored in-tree for Nexus (RFC 0002).
//!
//! This is `baileyrd/rush` refactored from a binary into a **library + thin
//! binary**. The library is the embeddable, testable shell core: it parses and
//! runs shell source and **never calls `std::process::exit`** — the `exit`
//! builtin sets a thread-local latch ([`exit_requested`]) that [`eval`] reads
//! and returns instead. The thin `src/main.rs` owns the single `process::exit`
//! and wires up argv dispatch.
//!
//! v0 scope: a REPL with persistent history, pipelines (`|`), redirections
//! (`>`, `>>`, `<`), and the builtins that must run in-process (`cd`, `exit`,
//! `pwd`). Quoting is handled by a small hand-written lexer so that
//! `echo "hello world"` is one argument. An expansion stage resolves `$VAR`,
//! `~`, `$(...)`, and filename globs (`*`, `?`, `[…]`) before a command runs,
//! and control operators (`&&`, `||`, `;`, `&`) sequence whole jobs. On Unix,
//! background and stopped jobs are managed with real job control (`fg`/`bg`/
//! `jobs`, Ctrl-Z); other platforms run foreground-only.
//!
//! ## Embedding
//!
//! When run inside a Nexus-owned PTY (the bundled sandbox shell), call
//! [`set_embedded(true)`](set_embedded) first: it disables rush's own
//! job-control terminal hand-off (`tcsetpgrp`/`setpgid`), since the PTY's
//! session leader — not rush's process group — owns the controlling terminal.
//! The thin binary sets this from the `NEXUS_EMBEDDED_SHELL=1` environment
//! variable.

mod arith;
mod builtins;
mod exec;
mod expand;
mod func;
mod glob;
#[cfg(unix)]
mod job;
mod lexer;
mod parser;
mod vars;

use std::path::PathBuf;

use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;

pub use parser::ParseError;

/// Parse and run a whole script (or `-c` string), returning the exit status.
///
/// This is the embeddable core (formerly `main.rs::run_source`). It never calls
/// `process::exit`: an `exit N` builtin sets the [`exit_requested`] latch, which
/// this function resolves into the returned status (and clears, so a subsequent
/// call starts clean).
#[must_use]
pub fn eval(src: &str) -> i32 {
    let status = run_source(src);
    match vars::exit_requested() {
        Some(code) => {
            vars::set_exit_requested(None);
            code
        }
        None => status,
    }
}

/// Like [`eval`], but resets all thread-local shell state first (see
/// [`reset_state`]) so the run cannot observe a prior eval's variables, `$?`,
/// positional parameters, or function definitions.
#[must_use]
pub fn eval_fresh(src: &str) -> i32 {
    vars::reset_state();
    eval(src)
}

/// How the binary was invoked, decoded from argv. Extracted so the dispatch is
/// testable without a tty (the REPL otherwise blocks on stdin).
#[derive(Debug, PartialEq, Eq)]
pub enum LaunchMode {
    /// Interactive REPL — `rush`, or an interactive flag (`-i`, `-l`, …).
    Repl,
    /// `rush -c "cmds" [name args…]`.
    Command {
        src: String,
        name: String,
        args: Vec<String>,
    },
    /// `rush FILE [args…]`.
    Script { path: String, args: Vec<String> },
}

/// Classify process arguments (`args[0]` is the program name). A first arg of
/// `-c` is a command string; **no arg or any leading-dash flag** (`-i`, `-l`, …)
/// selects the interactive REPL; anything else is a script file.
///
/// The leading-dash → REPL rule is what makes `nexus-rush -i` (how
/// `nexus-terminal` launches the bundled interactive shell) actually start the
/// REPL instead of trying to open a file named `-i`.
#[must_use]
pub fn classify_args(args: &[String]) -> LaunchMode {
    match args.get(1).map(String::as_str) {
        Some("-c") => LaunchMode::Command {
            src: args.get(2).cloned().unwrap_or_default(),
            name: args.get(3).cloned().unwrap_or_else(|| "rush".to_string()),
            args: args.get(4..).unwrap_or(&[]).to_vec(),
        },
        None => LaunchMode::Repl,
        Some(flag) if flag.starts_with('-') => LaunchMode::Repl,
        Some(file) => LaunchMode::Script {
            path: file.to_string(),
            args: args.get(2..).unwrap_or(&[]).to_vec(),
        },
    }
}

/// Set `$0` (shell/script name) and the positional parameters (`$1`…).
pub fn set_args(name: String, args: Vec<String>) {
    vars::set_args(name, args);
}

/// Clear all thread-local shell state: variables, `$?`, loop/return control, the
/// shell name, positional parameters, function definitions, and the `exit`
/// latch. Does **not** touch the embedded flag ([`set_embedded`]), which is a
/// host configuration rather than per-run shell state.
pub fn reset_state() {
    vars::reset_state();
}

/// Mark whether the shell is running embedded inside a Nexus-owned PTY. When
/// `true`, job-control terminal hand-off is disabled (see the module docs).
pub fn set_embedded(embedded: bool) {
    vars::set_embedded(embedded);
}

/// The pending `exit` request, if the `exit` builtin ran during the last
/// evaluation. [`eval`] already resolves and clears this; embedders driving the
/// REPL building blocks directly can poll it to know when to stop.
#[must_use]
pub fn exit_requested() -> Option<i32> {
    vars::exit_requested()
}

fn run_source(src: &str) -> i32 {
    match parser::parse(src) {
        Ok(list) => match exec::run_list(&list) {
            Ok(status) => status,
            Err(e) => {
                eprintln!("rush: {e}");
                1
            }
        },
        Err(e) => {
            eprintln!("rush: {e}");
            2
        }
    }
}

fn history_path() -> Option<PathBuf> {
    let mut p = PathBuf::from(std::env::var_os("HOME")?);
    p.push(".rush_history");
    Some(p)
}

fn prompt() -> String {
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "?".into());
    format!("{cwd} $ ")
}

/// Run the interactive REPL to completion, returning the shell's exit status.
///
/// Reads lines via rustyline (persistent history), accumulating until a complete
/// command parses — so an `if`/`while` can span several lines with a `> `
/// continuation prompt. `exit [n]` ends the loop with status `n`; Ctrl-D ends it
/// with the last command's status. This never calls `process::exit`; the thin
/// binary does, with the returned code.
#[must_use]
pub fn run_repl() -> i32 {
    match repl_inner() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("rush: {e}");
            1
        }
    }
}

fn repl_inner() -> rustyline::Result<i32> {
    let mut rl = DefaultEditor::new()?;
    let hist = history_path();
    if let Some(ref h) = hist {
        let _ = rl.load_history(h);
    }

    // Claim the terminal and set up signal handling for job control. A no-op for
    // terminal hand-off when embedded (see `job::init`).
    #[cfg(unix)]
    job::init();

    // Accumulates lines until a complete command is parsed.
    let mut buffer = String::new();
    // Assigned on every loop-exit path (`exit`, Ctrl-D, read error) below.
    let exit_code: i32;

    loop {
        // Report any background jobs that finished or stopped since last prompt.
        #[cfg(unix)]
        job::reap_background();

        let prompt = if buffer.is_empty() {
            prompt()
        } else {
            "> ".to_string()
        };
        match rl.readline(&prompt) {
            Ok(line) => {
                if buffer.is_empty() && line.trim().is_empty() {
                    continue;
                }
                rl.add_history_entry(&line)?;
                if !buffer.is_empty() {
                    buffer.push('\n');
                }
                buffer.push_str(&line);

                match parser::parse(&buffer) {
                    Ok(list) => {
                        if let Err(e) = exec::run_list(&list) {
                            eprintln!("rush: {e}");
                        }
                        buffer.clear();
                        // `exit` inside the REPL ends the loop with its status.
                        if let Some(code) = vars::exit_requested() {
                            vars::set_exit_requested(None);
                            exit_code = code;
                            break;
                        }
                    }
                    // A valid prefix: keep reading more lines.
                    Err(parser::ParseError::Incomplete) => {}
                    Err(parser::ParseError::Syntax(e)) => {
                        eprintln!("rush: {e}");
                        buffer.clear();
                    }
                }
            }
            // Ctrl-C: abandon the current (possibly multi-line) input.
            Err(ReadlineError::Interrupted) => {
                buffer.clear();
                continue;
            }
            // Ctrl-D on an empty line: exit with the last command's status.
            Err(ReadlineError::Eof) => {
                exit_code = vars::last_status();
                break;
            }
            Err(e) => {
                eprintln!("rush: {e}");
                exit_code = 1;
                break;
            }
        }
    }

    if let Some(ref h) = hist {
        let _ = rl.save_history(h);
    }
    Ok(exit_code)
}

#[cfg(test)]
mod lib_tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn classify_args_routes_interactive_flags_to_repl() {
        // Regression for the bundled-shell launch: `nexus-rush -i` must start the
        // REPL, not try to open a file named "-i". (RFC 0002 audit C1.)
        assert_eq!(classify_args(&argv(&["nexus-rush"])), LaunchMode::Repl);
        assert_eq!(
            classify_args(&argv(&["nexus-rush", "-i"])),
            LaunchMode::Repl
        );
        assert_eq!(
            classify_args(&argv(&["nexus-rush", "-l"])),
            LaunchMode::Repl
        );
        assert!(matches!(
            classify_args(&argv(&["nexus-rush", "-c", "echo hi"])),
            LaunchMode::Command { .. }
        ));
        assert!(matches!(
            classify_args(&argv(&["nexus-rush", "script.sh", "a"])),
            LaunchMode::Script { .. }
        ));
    }

    #[test]
    fn exit_builtin_returns_code_without_killing_process() {
        // If `exit` still called process::exit, the test harness would die here.
        assert_eq!(eval_fresh("exit 3"), 3);
        assert_eq!(eval_fresh("exit"), 0);
        assert_eq!(eval_fresh("true"), 0);
        assert_eq!(eval_fresh("false"), 1);
    }

    #[test]
    fn exit_latch_is_cleared_between_evals() {
        assert_eq!(eval_fresh("exit 7"), 7);
        // The latch must not leak into the next run.
        assert_eq!(exit_requested(), None);
        assert_eq!(eval_fresh("true"), 0);
    }

    #[test]
    fn eval_fresh_does_not_inherit_prior_variables() {
        let _ = eval_fresh("export NEXUS_RUSH_T=1");
        // A fresh eval starts with a clean variable table.
        assert_eq!(eval_fresh("test -n \"$NEXUS_RUSH_T\""), 1);
    }

    #[test]
    fn exit_inside_loop_stops_iteration() {
        // Without the exit-latch early-out in the loop executor this would spin
        // or run the body repeatedly; it must stop and return the exit code.
        assert_eq!(eval_fresh("for i in 1 2 3; do exit 4; done"), 4);
    }

    #[cfg(unix)]
    #[test]
    fn set_embedded_disables_job_control_init() {
        // In embedded mode, init() must not claim the terminal. We can't easily
        // assert syscalls, but we can assert it doesn't panic and leaves job
        // control disabled (foreground commands still run via the plain path).
        set_embedded(true);
        assert_eq!(eval_fresh("echo embedded >/dev/null"), 0);
        set_embedded(false);
    }
}
