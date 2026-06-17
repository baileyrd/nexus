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
use std::sync::OnceLock;
use std::time::Duration;

use nexus_types::SandboxPolicy;
use serde_json::Value;
use tokio::process::Command;
use tokio::sync::Semaphore;

/// Default wall-clock ceiling for a single headless subagent run. Matches the
/// `SESSION_RUN_TIMEOUT` the ai-runtime applies to an in-process session so an
/// isolated subagent isn't killed sooner than its shared-forge sibling.
pub const DEFAULT_SUBAGENT_TIMEOUT: Duration = Duration::from_secs(2 * 3600);

/// Env var overriding the `nexus` binary used to spawn isolated subagents
/// (RFC 0007 PR 4). The binary location is *install*-specific, not forge-
/// specific — the same forge opened from the CLI vs the Tauri shell needs a
/// different binary — so it's an env var rather than a per-forge `.forge`
/// setting. Frontends whose own `current_exe()` isn't the `nexus` CLI (shell,
/// MCP) set this so subagent isolation can locate the CLI.
pub const NEXUS_SUBAGENT_BIN_ENV: &str = "NEXUS_SUBAGENT_BIN";

/// Env var capping how many isolated subagents may run concurrently in this
/// process (RFC 0007 PR 4). Each is a full child `nexus` process, so the cap is
/// a host-resource guard; defaults to [`DEFAULT_MAX_CONCURRENT_SUBAGENTS`].
pub const NEXUS_SUBAGENT_MAX_CONCURRENT_ENV: &str = "NEXUS_SUBAGENT_MAX_CONCURRENT";

/// Default ceiling on concurrent isolated subagents per process. Conservative
/// because each is a full child runtime (its own storage engine + index), far
/// heavier than an in-process ai-runtime session.
pub const DEFAULT_MAX_CONCURRENT_SUBAGENTS: usize = 4;

/// Resolved location of the `nexus` binary used to spawn a subagent.
#[derive(Debug, Clone)]
pub struct SubagentRunner {
    nexus_bin: PathBuf,
    /// When set, the subagent is spawned through the `nexus-sandbox` helper
    /// under this confinement (RFC 0007 PR 3); `None` runs it directly.
    sandbox: Option<SandboxLaunch>,
}

/// OS-sandbox confinement for a spawned subagent: run it through the
/// `nexus-sandbox` helper sidecar (which self-confines, then execs the target)
/// under `policy`.
#[derive(Debug, Clone)]
struct SandboxLaunch {
    helper: PathBuf,
    policy: SandboxPolicy,
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
        Self {
            nexus_bin,
            sandbox: None,
        }
    }

    /// Spawn the subagent through the `nexus-sandbox` helper at `helper` under
    /// `policy` (RFC 0007 PR 3) instead of running the `nexus` binary directly.
    #[must_use]
    pub(crate) fn with_sandbox(mut self, helper: PathBuf, policy: SandboxPolicy) -> Self {
        self.sandbox = Some(SandboxLaunch { helper, policy });
        self
    }

    /// Resolve the `nexus` binary in precedence order: explicit `override_bin`,
    /// then the [`NEXUS_SUBAGENT_BIN_ENV`] env var, then the currently-running
    /// executable.
    ///
    /// The `current_exe()` fallback is correct only when the delegating process
    /// *is* the `nexus` CLI (CLI / TUI). The Tauri shell and the MCP server run
    /// a different binary and must set [`NEXUS_SUBAGENT_BIN_ENV`] (RFC 0007
    /// PR 4) so subagent isolation can locate the CLI.
    ///
    /// # Errors
    /// [`ResolveError::CurrentExe`] if neither an override nor the env var is
    /// set and `current_exe()` cannot be determined.
    pub fn resolve(override_bin: Option<PathBuf>) -> Result<Self, ResolveError> {
        resolve_binary(override_bin, std::env::var_os(NEXUS_SUBAGENT_BIN_ENV)).map(Self::with_binary)
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
    /// cannot shadow the explicit `--forge-path` the isolation relies on. When a
    /// sandbox is configured the child is launched through the `nexus-sandbox`
    /// helper (RFC 0007 PR 3).
    ///
    /// # Errors
    /// Propagates the `std::io::Error` from spawning the child (e.g. the
    /// resolved binary does not exist or is not executable).
    pub async fn run(&self, spec: &SubagentSpec) -> std::io::Result<SubagentOutcome> {
        let (program, argv) = self.spawn_invocation(spec)?;
        let mut cmd = Command::new(program);
        cmd.args(argv);
        cmd.env_remove("NEXUS_FORGE_PATH");
        run_capture(cmd, spec.timeout).await
    }

    /// Resolve the `(program, argv)` to spawn for `spec`: the `nexus` binary
    /// directly, or — when a sandbox is configured — the `nexus-sandbox` helper
    /// wrapping it (`[policy-json, cwd, "--", nexus-bin, agent-args…]`, with the
    /// worktree as the policy cwd). Pure; unit-tested without spawning.
    ///
    /// # Errors
    /// Returns `InvalidInput` if the sandbox policy cannot be serialized (it
    /// cannot in practice — `SandboxPolicy` is a plain derived `Serialize`).
    fn spawn_invocation(&self, spec: &SubagentSpec) -> std::io::Result<(PathBuf, Vec<OsString>)> {
        let target = Self::build_argv(spec);
        match &self.sandbox {
            None => Ok((self.nexus_bin.clone(), target)),
            Some(sb) => {
                let argv =
                    nexus_types::sandbox_argv(&sb.policy, &spec.forge_root, &self.nexus_bin, target)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
                Ok((sb.helper.clone(), argv))
            }
        }
    }
}

/// Derive the OS-sandbox policy for an isolated subagent from the parent
/// forge's policy (RFC 0007 PR 3): confine writes to the subagent's `worktree`
/// (a `workspace-write` policy), inheriting the parent's network posture. The
/// caller skips this entirely for a `danger-full-access` parent (operator opted
/// out of sandboxing).
#[must_use]
pub(crate) fn derive_subagent_policy(parent: &SandboxPolicy, worktree: &Path) -> SandboxPolicy {
    SandboxPolicy::WorkspaceWrite {
        writable_roots: vec![worktree.to_path_buf()],
        network_access: parent.has_full_network_access(),
        exclude_tmpdir_env_var: false,
        exclude_slash_tmp: false,
    }
}

/// Locate the `nexus-sandbox` helper sidecar next to the `nexus` binary, or
/// `None` if it isn't present (e.g. not built, or a host without the helper).
/// Callers fall back to an unconfined spawn when this returns `None`.
#[must_use]
pub(crate) fn locate_sandbox_helper(nexus_bin: &Path) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "nexus-sandbox.exe"
    } else {
        "nexus-sandbox"
    };
    let helper = nexus_bin.with_file_name(name);
    helper.exists().then_some(helper)
}

/// Pure precedence core of [`SubagentRunner::resolve`]: `override_bin`, then the
/// env var value, then `current_exe()`. Split out so the precedence is
/// unit-testable without mutating the process environment.
fn resolve_binary(
    override_bin: Option<PathBuf>,
    env_bin: Option<OsString>,
) -> Result<PathBuf, ResolveError> {
    if let Some(p) = override_bin {
        return Ok(p);
    }
    if let Some(e) = env_bin {
        if !e.is_empty() {
            return Ok(PathBuf::from(e));
        }
    }
    std::env::current_exe().map_err(ResolveError::CurrentExe)
}

/// Parse the [`NEXUS_SUBAGENT_MAX_CONCURRENT_ENV`] value: a positive integer, or
/// [`DEFAULT_MAX_CONCURRENT_SUBAGENTS`] when unset / empty / non-positive /
/// unparseable. Pure so it's unit-testable without env mutation.
#[must_use]
fn parse_max_concurrent(raw: Option<&str>) -> usize {
    raw.and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_MAX_CONCURRENT_SUBAGENTS)
}

/// Process-wide semaphore bounding concurrent isolated subagents (RFC 0007
/// PR 4). Initialised once from [`NEXUS_SUBAGENT_MAX_CONCURRENT_ENV`]; the limit
/// spans every parent session in the process because the constraint is the host
/// (each subagent is a full child `nexus` process).
fn subagent_semaphore() -> &'static Semaphore {
    static SEM: OnceLock<Semaphore> = OnceLock::new();
    SEM.get_or_init(|| {
        let limit = parse_max_concurrent(
            std::env::var(NEXUS_SUBAGENT_MAX_CONCURRENT_ENV)
                .ok()
                .as_deref(),
        );
        Semaphore::new(limit)
    })
}

/// Acquire a slot to run one isolated subagent, blocking until one frees up.
/// Hold the returned permit for the lifetime of the spawn + merge; dropping it
/// (including on an early return) releases the slot.
pub(crate) async fn acquire_subagent_slot() -> tokio::sync::SemaphorePermit<'static> {
    subagent_semaphore()
        .acquire()
        .await
        .expect("subagent semaphore is never closed")
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

    // ── spawn_invocation + sandbox (RFC 0007 PR 3) ───────────────────────────

    #[test]
    fn spawn_invocation_plain_runs_nexus_directly() {
        let runner = SubagentRunner::with_binary(PathBuf::from("/opt/nexus"));
        let (program, argv) = runner.spawn_invocation(&spec("do it")).unwrap();
        assert_eq!(program, PathBuf::from("/opt/nexus"));
        assert_eq!(argv, SubagentRunner::build_argv(&spec("do it")));
    }

    #[test]
    fn spawn_invocation_sandboxed_wraps_helper() {
        let runner = SubagentRunner::with_binary(PathBuf::from("/opt/nexus")).with_sandbox(
            PathBuf::from("/opt/nexus-sandbox"),
            SandboxPolicy::new_workspace_write(vec![PathBuf::from("/tmp/wt")]),
        );
        let (program, argv) = runner.spawn_invocation(&spec("do it")).unwrap();
        assert_eq!(program, PathBuf::from("/opt/nexus-sandbox"));
        // Helper argv: [policy-json, cwd, "--", nexus-bin, agent, run, …].
        let as_str: Vec<String> = argv
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert!(
            as_str[0].contains("workspace-write"),
            "policy json comes first: {}",
            as_str[0]
        );
        assert_eq!(as_str[1], "/tmp/wt", "cwd is the worktree (spec.forge_root)");
        assert_eq!(as_str[2], "--");
        assert_eq!(as_str[3], "/opt/nexus", "wrapped program is the nexus binary");
        assert_eq!(as_str[4], "agent");
        assert_eq!(as_str[5], "run");
    }

    #[test]
    fn derive_subagent_policy_scopes_writes_to_worktree() {
        let wt = Path::new("/forge/.forge/worktrees/subagent-abc");
        match derive_subagent_policy(&SandboxPolicy::ReadOnly, wt) {
            SandboxPolicy::WorkspaceWrite {
                writable_roots,
                network_access,
                ..
            } => {
                assert_eq!(writable_roots, vec![wt.to_path_buf()]);
                assert!(!network_access, "a read-only parent grants no network");
            }
            other => panic!("expected workspace-write, got {other:?}"),
        }
    }

    #[test]
    fn derive_subagent_policy_inherits_parent_network() {
        let wt = Path::new("/wt");
        let net_parent = SandboxPolicy::WorkspaceWrite {
            writable_roots: vec![PathBuf::from("/elsewhere")],
            network_access: true,
            exclude_tmpdir_env_var: false,
            exclude_slash_tmp: false,
        };
        assert!(derive_subagent_policy(&net_parent, wt).has_full_network_access());
        assert!(derive_subagent_policy(&SandboxPolicy::DangerFullAccess, wt).has_full_network_access());
    }

    #[test]
    fn locate_sandbox_helper_absent_is_none() {
        // A binary in a temp dir with no sibling helper resolves to None.
        let dir = std::env::temp_dir().join("nexus-subagent-test-no-helper");
        let bin = dir.join("nexus");
        assert!(locate_sandbox_helper(&bin).is_none());
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

    #[test]
    fn resolve_binary_precedence_override_then_env_then_exe() {
        // Explicit override beats the env var.
        let r = resolve_binary(
            Some(PathBuf::from("/a/nexus")),
            Some(OsString::from("/b/nexus")),
        )
        .unwrap();
        assert_eq!(r, PathBuf::from("/a/nexus"));
        // Env var used when there's no override.
        let r = resolve_binary(None, Some(OsString::from("/b/nexus"))).unwrap();
        assert_eq!(r, PathBuf::from("/b/nexus"));
        // An empty env value is ignored, falling through to current_exe.
        let r = resolve_binary(None, Some(OsString::new())).unwrap();
        assert!(r.is_absolute() || r.exists());
        // Neither set → current_exe.
        let r = resolve_binary(None, None).unwrap();
        assert!(r.is_absolute() || r.exists());
    }

    #[test]
    fn parse_max_concurrent_defaults_and_overrides() {
        assert_eq!(parse_max_concurrent(None), DEFAULT_MAX_CONCURRENT_SUBAGENTS);
        assert_eq!(parse_max_concurrent(Some("")), DEFAULT_MAX_CONCURRENT_SUBAGENTS);
        assert_eq!(parse_max_concurrent(Some("0")), DEFAULT_MAX_CONCURRENT_SUBAGENTS);
        assert_eq!(parse_max_concurrent(Some("nope")), DEFAULT_MAX_CONCURRENT_SUBAGENTS);
        assert_eq!(parse_max_concurrent(Some("  8 ")), 8);
        assert_eq!(parse_max_concurrent(Some("1")), 1);
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
