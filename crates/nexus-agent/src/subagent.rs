//! Headless subagent process spawning (RFC 0007, PR 1).
//!
//! Option A subagent isolation (RFC 0006 → 0007) runs a delegated subagent as a
//! *separate* `nexus` process pointed at an isolated forge root, then merges its
//! delta back into the parent. This module is the lowest layer of that
//! machinery and nothing more: resolve the `nexus` binary, build the
//! `agent run` argv, spawn the child with a timeout, and capture its transcript
//! + exit status.
//!
//! It deliberately does **not** create git worktrees, commit, merge, derive a
//! sandbox policy, or wire into the `delegate` handler — those land in later PRs
//! (RFC 0007 §Phasing). Keeping the spawn mechanism isolated makes it a small,
//! reviewable, side-effect-light building block: the only effect is the child
//! process it runs.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use tokio::process::Command;

/// Default wall-clock ceiling for a single headless subagent run. Matches the
/// `SESSION_RUN_TIMEOUT` the ai-runtime applies to an in-process session so an
/// isolated subagent isn't killed sooner than its shared-forge sibling.
pub const DEFAULT_SUBAGENT_TIMEOUT: Duration = Duration::from_secs(2 * 3600);

/// Resolved location of the `nexus` binary used to spawn a subagent.
#[derive(Debug, Clone)]
pub struct SubagentRunner {
    nexus_bin: PathBuf,
}

/// Failure resolving the `nexus` binary path.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    /// `std::env::current_exe()` failed and no explicit override was given.
    #[error("cannot locate the `nexus` binary to spawn a subagent: {0}")]
    CurrentExe(#[source] std::io::Error),
}

/// One headless subagent run: which forge to run against, the goal, and the
/// wall-clock ceiling.
#[derive(Debug, Clone)]
pub struct SubagentSpec {
    /// Forge root the child runs against — its `--forge-path`. For isolation
    /// (PR 2) this is a git-worktree checkout; on its own a `SubagentSpec` does
    /// not care how the directory came to be.
    pub forge_root: PathBuf,
    /// Natural-language goal handed to `agent run`.
    pub goal: String,
    /// Optional archetype (`writer` / `coder` / …); `None` uses the default.
    pub archetype: Option<String>,
    /// Kill the child if it runs longer than this.
    pub timeout: Duration,
}

impl SubagentSpec {
    /// A spec for `goal` against `forge_root` with the default timeout and no
    /// archetype override.
    #[must_use]
    pub fn new(forge_root: PathBuf, goal: impl Into<String>) -> Self {
        Self {
            forge_root,
            goal: goal.into(),
            archetype: None,
            timeout: DEFAULT_SUBAGENT_TIMEOUT,
        }
    }
}

/// Captured result of a headless subagent process.
#[derive(Debug, Clone)]
pub struct SubagentOutcome {
    /// Process exit code, or `None` if the child was killed by a signal or by
    /// the timeout.
    pub exit_code: Option<i32>,
    /// `true` when the child was killed because it exceeded `spec.timeout`.
    pub timed_out: bool,
    /// Raw captured stdout — the `--format json` transcript on the happy path.
    /// Empty on timeout (the partial buffer is discarded with the killed child).
    pub stdout: String,
    /// Raw captured stderr — tracing diagnostics / approval banners. Kept for
    /// surfacing failures; never parsed.
    pub stderr: String,
    /// Best-effort parse of `stdout` as the session transcript JSON. `None`
    /// when the child produced no parseable object (crash, timeout, or a
    /// non-JSON banner). Callers that need the outcome must treat `None` as
    /// failure.
    pub transcript: Option<Value>,
}

impl SubagentOutcome {
    /// `true` when the child exited `0`, was not timed out, and emitted a
    /// parseable transcript.
    #[must_use]
    pub fn succeeded(&self) -> bool {
        self.exit_code == Some(0) && !self.timed_out && self.transcript.is_some()
    }
}

impl SubagentRunner {
    /// Use an explicit path to the `nexus` binary.
    #[must_use]
    pub fn with_binary(nexus_bin: PathBuf) -> Self {
        Self { nexus_bin }
    }

    /// Resolve the `nexus` binary: prefer `override_bin`, otherwise fall back to
    /// the currently-running executable.
    ///
    /// The `current_exe()` fallback is correct only when the delegating process
    /// *is* the `nexus` CLI (CLI / TUI). The Tauri shell and the MCP server run
    /// a different binary and must pass an explicit override — RFC 0007 promotes
    /// that override to an `agent.subagent.nexus_bin` setting in PR 4.
    ///
    /// # Errors
    /// [`ResolveError::CurrentExe`] if no override is given and `current_exe()`
    /// cannot be determined.
    pub fn resolve(override_bin: Option<PathBuf>) -> Result<Self, ResolveError> {
        match override_bin {
            Some(p) => Ok(Self::with_binary(p)),
            None => std::env::current_exe()
                .map(Self::with_binary)
                .map_err(ResolveError::CurrentExe),
        }
    }

    /// The resolved `nexus` binary path.
    #[must_use]
    pub fn binary(&self) -> &Path {
        &self.nexus_bin
    }

    /// Build the argv (everything after the binary) for a headless run.
    ///
    /// Shape: `agent run --forge-path <root> --format json [--archetype <a>] --
    /// <goal>`. `--forge-path` and `--format` are global flags, so their
    /// placement before the subcommand positional is valid; `--format json`
    /// makes `agent run` emit the raw session transcript for capture. The
    /// trailing `--` stops flag parsing so a goal that begins with `-` is taken
    /// as the positional rather than mistaken for an option. `auto_approve` is
    /// the `agent run` default (no `--interactive`), so no flag is needed.
    ///
    /// Pure and total — unit-tested without spawning anything.
    #[must_use]
    pub fn build_argv(spec: &SubagentSpec) -> Vec<OsString> {
        let mut argv: Vec<OsString> = vec![
            OsString::from("agent"),
            OsString::from("run"),
            OsString::from("--forge-path"),
            spec.forge_root.clone().into_os_string(),
            OsString::from("--format"),
            OsString::from("json"),
        ];
        if let Some(archetype) = &spec.archetype {
            argv.push(OsString::from("--archetype"));
            argv.push(OsString::from(archetype));
        }
        argv.push(OsString::from("--"));
        argv.push(OsString::from(&spec.goal));
        argv
    }

    /// Spawn the headless subagent and capture its result, killing it if it
    /// exceeds `spec.timeout`.
    ///
    /// stdin is closed (a headless run never prompts), stdout / stderr are
    /// captured. `NEXUS_FORGE_PATH` is cleared from the child environment so it
    /// cannot shadow the explicit `--forge-path` the isolation relies on.
    ///
    /// # Errors
    /// Propagates the `std::io::Error` from spawning the child (e.g. the
    /// resolved binary does not exist or is not executable).
    pub async fn run(&self, spec: &SubagentSpec) -> std::io::Result<SubagentOutcome> {
        let mut cmd = Command::new(&self.nexus_bin);
        cmd.args(Self::build_argv(spec));
        cmd.env_remove("NEXUS_FORGE_PATH");
        run_capture(cmd, spec.timeout).await
    }
}

/// Spawn `cmd`, capture stdout/stderr, and enforce `timeout` by killing the
/// child on expiry.
///
/// Factored out of [`SubagentRunner::run`] so the spawn / timeout / capture
/// mechanism can be exercised against an arbitrary command without a real
/// `nexus` binary. `kill_on_drop(true)` means a timeout — which drops the
/// in-flight `wait_with_output` future — sends `SIGKILL` to the child; its
/// partial output is discarded with it.
async fn run_capture(
    mut cmd: Command,
    timeout: Duration,
) -> std::io::Result<SubagentOutcome> {
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);

    let child = cmd.spawn()?;
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(result) => {
            let output = result?;
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            let transcript = parse_transcript(&stdout);
            Ok(SubagentOutcome {
                exit_code: output.status.code(),
                timed_out: false,
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                stdout,
                transcript,
            })
        }
        Err(_elapsed) => Ok(SubagentOutcome {
            exit_code: None,
            timed_out: true,
            stdout: String::new(),
            stderr: String::new(),
            transcript: None,
        }),
    }
}

/// Best-effort-extract the session transcript object from captured stdout.
///
/// The happy path is a single pretty-printed JSON object (`agent run
/// --format json`). The child's tracing subscriber writes to stdout at `WARN`
/// by default, so a stray warning line can precede the JSON; the fallback skips
/// to the first `{` and parses the remainder. Returns `None` when nothing
/// parses.
fn parse_transcript(stdout: &str) -> Option<Value> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value);
    }
    let start = trimmed.find('{')?;
    serde_json::from_str::<Value>(trimmed[start..].trim()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(goal: &str) -> SubagentSpec {
        SubagentSpec::new(PathBuf::from("/tmp/wt"), goal)
    }

    // ── build_argv (pure) ────────────────────────────────────────────────────

    #[test]
    fn build_argv_minimal_shape() {
        let argv = SubagentRunner::build_argv(&spec("write the notes"));
        let as_str: Vec<String> = argv
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            as_str,
            vec![
                "agent",
                "run",
                "--forge-path",
                "/tmp/wt",
                "--format",
                "json",
                "--",
                "write the notes",
            ]
        );
    }

    #[test]
    fn build_argv_includes_archetype_when_set() {
        let mut s = spec("do it");
        s.archetype = Some("coder".to_string());
        let argv = SubagentRunner::build_argv(&s);
        let as_str: Vec<String> = argv
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        let pos = as_str.iter().position(|a| a == "--archetype").expect("flag");
        assert_eq!(as_str[pos + 1], "coder");
        // archetype must precede the `--` goal separator.
        let sep = as_str.iter().position(|a| a == "--").expect("separator");
        assert!(pos < sep, "archetype flag must come before the `--` guard");
    }

    #[test]
    fn build_argv_dash_guard_keeps_leading_dash_goal_positional() {
        let argv = SubagentRunner::build_argv(&spec("-x marks the spot"));
        let as_str: Vec<String> = argv
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        // The goal is the last element and is preceded by `--`, so clap can't
        // mistake its leading dash for a flag.
        assert_eq!(as_str.last().map(String::as_str), Some("-x marks the spot"));
        let sep = as_str.iter().position(|a| a == "--").expect("separator");
        assert_eq!(sep, as_str.len() - 2, "`--` must immediately precede goal");
    }

    // ── resolve ──────────────────────────────────────────────────────────────

    #[test]
    fn resolve_prefers_explicit_override() {
        let runner = SubagentRunner::resolve(Some(PathBuf::from("/opt/nexus")))
            .expect("override always resolves");
        assert_eq!(runner.binary(), Path::new("/opt/nexus"));
    }

    #[test]
    fn resolve_falls_back_to_current_exe() {
        // In the test binary `current_exe()` is available, so the fallback
        // resolves to *some* path (the test runner) rather than erroring.
        let runner = SubagentRunner::resolve(None).expect("current_exe resolves in tests");
        assert!(runner.binary().is_absolute() || runner.binary().exists());
    }

    // ── parse_transcript (pure) ──────────────────────────────────────────────

    #[test]
    fn parse_transcript_clean_json() {
        let out = r#"{"id":"s1","outcome":"complete","rounds":[]}"#;
        let v = parse_transcript(out).expect("parses clean json");
        assert_eq!(v["outcome"], "complete");
    }

    #[test]
    fn parse_transcript_pretty_with_leading_log_line() {
        // A WARN line on stdout (tracing default) precedes the pretty JSON.
        let out = "WARN nexus: something\n{\n  \"outcome\": \"complete\"\n}";
        let v = parse_transcript(out).expect("recovers json after a log line");
        assert_eq!(v["outcome"], "complete");
    }

    #[test]
    fn parse_transcript_empty_is_none() {
        assert!(parse_transcript("").is_none());
        assert!(parse_transcript("   \n  ").is_none());
    }

    #[test]
    fn parse_transcript_non_json_is_none() {
        assert!(parse_transcript("error: forge not found").is_none());
    }

    // ── succeeded ────────────────────────────────────────────────────────────

    #[test]
    fn succeeded_requires_zero_exit_and_transcript() {
        let base = SubagentOutcome {
            exit_code: Some(0),
            timed_out: false,
            stdout: String::new(),
            stderr: String::new(),
            transcript: Some(serde_json::json!({"outcome": "complete"})),
        };
        assert!(base.succeeded());

        let nonzero = SubagentOutcome {
            exit_code: Some(1),
            ..base.clone()
        };
        assert!(!nonzero.succeeded());

        let timed_out = SubagentOutcome {
            timed_out: true,
            ..base.clone()
        };
        assert!(!timed_out.succeeded());

        let no_transcript = SubagentOutcome {
            transcript: None,
            ..base
        };
        assert!(!no_transcript.succeeded());
    }

    // ── live spawn (ignored: CI sandbox forbids spawning processes) ───────────

    /// Exercises the real spawn / timeout / kill path against a portable
    /// command. Ignored by default because the CI sandbox forbids spawning
    /// external binaries (see the note in `nexus-cli`'s `main.rs` tests); run
    /// locally with `cargo test -p nexus-agent -- --ignored`.
    #[tokio::test]
    #[ignore = "spawns an external process; CI sandbox forbids it"]
    async fn run_capture_times_out_and_kills() {
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let outcome = run_capture(cmd, Duration::from_millis(100))
            .await
            .expect("spawn sleep");
        assert!(outcome.timed_out);
        assert_eq!(outcome.exit_code, None);
        assert!(outcome.transcript.is_none());
    }

    #[tokio::test]
    #[ignore = "spawns an external process; CI sandbox forbids it"]
    async fn run_capture_captures_stdout_and_exit() {
        let mut cmd = Command::new("printf");
        cmd.arg(r#"{"outcome":"complete"}"#);
        let outcome = run_capture(cmd, Duration::from_secs(5))
            .await
            .expect("spawn printf");
        assert!(!outcome.timed_out);
        assert_eq!(outcome.exit_code, Some(0));
        assert_eq!(outcome.transcript.expect("json")["outcome"], "complete");
    }
}
