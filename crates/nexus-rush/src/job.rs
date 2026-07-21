//! Unix job control: process groups, terminal hand-off, and `fg`/`bg`/`jobs`.
//!
//! This module follows the structure of the classic glibc "Implementing a Job
//! Control Shell" example:
//!
//!   * At startup the shell ignores the job-control signals (`SIGINT`,
//!     `SIGTSTP`, …) and puts itself in its own process group that owns the
//!     terminal. Each child *resets* those signals to default and joins the
//!     job's process group before `exec`, so keystrokes like Ctrl-C / Ctrl-Z
//!     are delivered to the foreground job, not the shell.
//!   * A foreground job is handed the terminal (`tcsetpgrp`); the shell waits
//!     for it to exit *or stop* (`WUNTRACED`) and then reclaims the terminal.
//!   * A background job (`&`) is left in its own group without the terminal.
//!     Finished/stopped background jobs are reported at the next prompt.
//!
//! It is compiled only on Unix; the rest of the shell degrades to a plain
//! spawn-and-wait on other platforms.

use std::cell::RefCell;
use std::os::unix::process::CommandExt;
use std::process::Stdio;

use libc::{c_int, pid_t};

use crate::exec::Pipeline;

#[derive(Clone, Copy, PartialEq, Eq)]
enum JobState {
    Running,
    Stopped,
    Done,
}

struct JobEntry {
    id: usize,
    pgid: pid_t,
    pids: Vec<pid_t>,
    live: usize,
    cmd: String,
    state: JobState,
    notified: bool,
}

#[derive(Default)]
struct State {
    shell_pgid: pid_t,
    job_control: bool,
    jobs: Vec<JobEntry>,
    next_id: usize,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// The job-control signals the shell ignores and children reset to default.
const JOB_SIGNALS: [c_int; 5] = [
    libc::SIGINT,
    libc::SIGQUIT,
    libc::SIGTSTP,
    libc::SIGTTIN,
    libc::SIGTTOU,
];

/// Set up job control: only when stdin is a terminal **and** the shell is not
/// embedded in a Nexus-owned PTY. Idempotent enough to call once at startup.
///
/// When embedded (RFC 0002), the PTY's session leader — not rush's process group
/// — owns the controlling terminal, so rush must not ignore the job-control
/// signals, become a group leader, or `tcsetpgrp`. Leaving `job_control` off
/// routes foreground commands through the plain spawn-and-wait path (see
/// `exec::run_foreground`), which keeps children in the shell's group sharing the
/// terminal.
pub fn init() {
    let interactive = unsafe { libc::isatty(libc::STDIN_FILENO) } == 1;
    let enable = interactive && !crate::vars::embedded();
    let pid = unsafe { libc::getpid() };

    if enable {
        unsafe {
            for &sig in &JOB_SIGNALS {
                libc::signal(sig, libc::SIG_IGN);
            }
            // Become a process-group leader.
            libc::setpgid(pid, pid);
        }
        // ...and take the terminal.
        tcsetpgrp_impl(pid);
    }

    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.shell_pgid = pid;
        s.job_control = enable;
    });
}

/// Spawn every stage of a pipeline into a single new process group, returning
/// `(pgid, pids)`. Children reset signal dispositions and join the group before
/// `exec`; the parent also calls `setpgid` to avoid racing terminal hand-off.
fn spawn_pipeline(pipeline: &Pipeline) -> Result<(pid_t, Vec<pid_t>), String> {
    let n = pipeline.commands.len();
    let mut pids = Vec::with_capacity(n);
    let mut pgid: pid_t = 0;
    let mut prev_stdout: Option<Stdio> = None;

    for (i, cmd) in pipeline.commands.iter().enumerate() {
        let is_last = i == n - 1;
        let mut command = crate::exec::build_stage(cmd, prev_stdout.take(), is_last, false)?;

        // `0` for the leader means "new group whose id is my pid".
        let target_pgid = pgid;
        unsafe {
            command.pre_exec(move || {
                libc::setpgid(0, target_pgid);
                for &sig in &JOB_SIGNALS {
                    libc::signal(sig, libc::SIG_DFL);
                }
                libc::signal(libc::SIGCHLD, libc::SIG_DFL);
                Ok(())
            });
        }

        let mut child = command
            .spawn()
            .map_err(|e| format!("{}: {e}", cmd.argv[0]))?;
        crate::exec::feed_heredoc(&mut child, cmd);
        let pid = child.id() as pid_t;
        if i == 0 {
            pgid = pid;
        }
        unsafe {
            libc::setpgid(pid, pgid);
        }

        if !is_last {
            prev_stdout = child.stdout.take().map(Stdio::from);
        }
        pids.push(pid);
        // We reap via `waitpid`, so let the std handle drop (its Drop neither
        // waits nor kills on Unix).
    }

    Ok((pgid, pids))
}

/// Run a pipeline in the foreground, returning its exit status. If it stops
/// (Ctrl-Z), it is added to the job table and we return `128 + SIGTSTP`.
pub fn run_foreground(pipeline: &Pipeline) -> Result<i32, String> {
    let (pgid, pids) = spawn_pipeline(pipeline)?;
    give_terminal(pgid);

    let result = wait_pgid(pgid, &pids);
    reclaim_terminal();

    Ok(match result {
        Wait::Done(code) => code,
        Wait::Stopped(code) => {
            let cmd = crate::exec::pipeline_text(pipeline);
            let id = add_job(pgid, &pids, &cmd, JobState::Stopped);
            eprintln!("\n[{id}]+  Stopped\t{cmd}");
            code
        }
    })
}

/// Run a pipeline in the background: record it and print `[id] pgid`.
pub fn run_background(pipeline: &Pipeline) -> Result<(), String> {
    let (pgid, pids) = spawn_pipeline(pipeline)?;
    let cmd = crate::exec::pipeline_text(pipeline);
    let id = add_job(pgid, &pids, &cmd, JobState::Running);
    println!("[{id}] {pgid}");
    Ok(())
}

enum Wait {
    Done(i32),
    Stopped(i32),
}

/// Wait for a process group to finish or stop, reaping its members.
fn wait_pgid(pgid: pid_t, pids: &[pid_t]) -> Wait {
    let last = *pids.last().expect("pipeline has at least one stage");
    let mut live: usize = pids.len();
    let mut last_code = 0;

    while live > 0 {
        let mut status: c_int = 0;
        let wpid = unsafe { libc::waitpid(-pgid, &mut status, libc::WUNTRACED) };
        if wpid <= 0 {
            break; // -1 (ECHILD) or 0: nothing left to wait for
        }
        if wifstopped(status) {
            return Wait::Stopped(128 + libc::WSTOPSIG(status) as i32);
        }
        // Exited or killed by a signal.
        live -= 1;
        if wpid == last {
            last_code = exit_code(status);
        }
    }

    Wait::Done(last_code)
}

/// Reap finished/stopped/continued background jobs without blocking, reporting
/// state changes. Called once before each prompt.
///
/// This runs **regardless of `job_control`**: a shell embedded in a Nexus-owned
/// PTY (RFC 0002) keeps job control off, but `&` still spawns real children via
/// [`run_background`]. Without reaping them here those children would linger as
/// zombies for the life of the embedded shell. Foreground commands in embedded
/// mode are already waited synchronously (see `exec::run_foreground`), so the
/// non-blocking `waitpid(-1)` below only collects background-job members.
pub fn reap_background() {
    loop {
        let mut status: c_int = 0;
        let flags = libc::WNOHANG | libc::WUNTRACED | libc::WCONTINUED;
        let wpid = unsafe { libc::waitpid(-1, &mut status, flags) };
        if wpid <= 0 {
            break; // 0: no change; -1: no children
        }
        update_by_pid(wpid, status);
    }

    notify_and_prune();
}

// ---- builtins: jobs / fg / bg ------------------------------------------------

/// Dispatch the job-control builtins. Returns `Some(code)` if handled.
pub fn builtin(argv: &[String]) -> Option<i32> {
    match argv.first().map(String::as_str)? {
        "jobs" => Some(jobs_cmd()),
        "fg" => Some(fg_cmd(argv)),
        "bg" => Some(bg_cmd(argv)),
        "kill" => Some(kill_cmd(argv)),
        _ => None,
    }
}

/// `kill [-SIG] %job|pid …` — signal a job (by `%n`) or process. The default
/// signal is `TERM`; `-9`, `-KILL`, `-SIGKILL`, etc. are accepted.
fn kill_cmd(argv: &[String]) -> i32 {
    let mut sig = libc::SIGTERM;
    let mut start = 1;
    if let Some(first) = argv.get(1).and_then(|a| a.strip_prefix('-')) {
        match parse_signal(first) {
            Some(s) => {
                sig = s;
                start = 2;
            }
            None => {
                eprintln!("kill: {first}: invalid signal specification");
                return 1;
            }
        }
    }
    if argv.len() <= start {
        eprintln!("kill: usage: kill [-signal] %job | pid ...");
        return 1;
    }

    let mut status = 0;
    for target in &argv[start..] {
        if let Some(spec) = target.strip_prefix('%') {
            match spec.parse::<usize>().ok().and_then(job_pgid) {
                Some(pgid) => unsafe {
                    libc::killpg(pgid, sig);
                },
                None => {
                    eprintln!("kill: %{spec}: no such job");
                    status = 1;
                }
            }
        } else if let Ok(pid) = target.parse::<pid_t>() {
            unsafe {
                libc::kill(pid, sig);
            }
        } else {
            eprintln!("kill: {target}: arguments must be job or process IDs");
            status = 1;
        }
    }
    status
}

fn job_pgid(id: usize) -> Option<pid_t> {
    STATE.with(|s| s.borrow().jobs.iter().find(|j| j.id == id).map(|j| j.pgid))
}

fn parse_signal(name: &str) -> Option<c_int> {
    if let Ok(n) = name.parse::<c_int>() {
        return Some(n);
    }
    let upper = name.to_ascii_uppercase();
    match upper.strip_prefix("SIG").unwrap_or(&upper) {
        "TERM" => Some(libc::SIGTERM),
        "KILL" => Some(libc::SIGKILL),
        "INT" => Some(libc::SIGINT),
        "HUP" => Some(libc::SIGHUP),
        "QUIT" => Some(libc::SIGQUIT),
        "STOP" => Some(libc::SIGSTOP),
        "CONT" => Some(libc::SIGCONT),
        _ => None,
    }
}

fn jobs_cmd() -> i32 {
    STATE.with(|s| {
        for j in &s.borrow().jobs {
            println!("[{}]  {}\t{}", j.id, state_label(j.state), j.cmd);
        }
    });
    0
}

fn fg_cmd(argv: &[String]) -> i32 {
    let mut job = match take_selected(argv) {
        Some(j) => j,
        None => {
            eprintln!("fg: no current job");
            return 1;
        }
    };

    println!("{}", job.cmd);
    give_terminal(job.pgid);
    unsafe {
        libc::killpg(job.pgid, libc::SIGCONT);
    }

    let result = wait_pgid(job.pgid, &job.pids);
    reclaim_terminal();

    match result {
        Wait::Done(code) => code,
        Wait::Stopped(code) => {
            job.state = JobState::Stopped;
            eprintln!("\n[{}]+  Stopped\t{}", job.id, job.cmd);
            reinsert(job);
            code
        }
    }
}

fn bg_cmd(argv: &[String]) -> i32 {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let idx = match select_index(&s, argv) {
            Some(i) => i,
            None => {
                eprintln!("bg: no current job");
                return 1;
            }
        };
        let job = &mut s.jobs[idx];
        job.state = JobState::Running;
        unsafe {
            libc::killpg(job.pgid, libc::SIGCONT);
        }
        println!("[{}] {} &", job.id, job.cmd);
        0
    })
}

// ---- job table helpers -------------------------------------------------------

fn add_job(pgid: pid_t, pids: &[pid_t], cmd: &str, state: JobState) -> usize {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.next_id += 1;
        let id = s.next_id;
        s.jobs.push(JobEntry {
            id,
            pgid,
            pids: pids.to_vec(),
            live: pids.len(),
            cmd: cmd.to_string(),
            state,
            notified: false,
        });
        id
    })
}

fn reinsert(job: JobEntry) {
    STATE.with(|s| s.borrow_mut().jobs.push(job));
}

/// Remove and return the job selected by `argv` (a `%n`/`n` spec, or the most
/// recent job when no spec is given).
fn take_selected(argv: &[String]) -> Option<JobEntry> {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let idx = select_index(&s, argv)?;
        Some(s.jobs.remove(idx))
    })
}

fn select_index(s: &State, argv: &[String]) -> Option<usize> {
    match argv.get(1) {
        Some(spec) => {
            let n: usize = spec.trim_start_matches('%').parse().ok()?;
            s.jobs.iter().position(|j| j.id == n)
        }
        // Most recent job that isn't already finished.
        None => s.jobs.iter().rposition(|j| j.state != JobState::Done),
    }
}

fn update_by_pid(wpid: pid_t, status: c_int) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let Some(job) = s.jobs.iter_mut().find(|j| j.pids.contains(&wpid)) else {
            return;
        };
        if wifstopped(status) {
            job.state = JobState::Stopped;
            job.notified = false;
        } else if wifcontinued(status) {
            job.state = JobState::Running;
        } else {
            // Exited or signaled.
            job.live = job.live.saturating_sub(1);
            if job.live == 0 {
                job.state = JobState::Done;
            }
        }
    });
}

fn notify_and_prune() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        for j in &mut s.jobs {
            if j.state == JobState::Done && !j.notified {
                eprintln!("[{}]  Done\t{}", j.id, j.cmd);
                j.notified = true;
            }
        }
        s.jobs.retain(|j| j.state != JobState::Done);
    });
}

// ---- terminal + status helpers ----------------------------------------------

/// Hand the controlling terminal's foreground process group to `pgid`
/// (`tcsetpgrp`), unconditionally — callers gate on [`job_control_enabled`]
/// themselves (see [`give_terminal`]) or, in [`init`]'s case, haven't set
/// that flag yet.
///
/// On Linux this goes through `platform::term::JobControlTerminal`
/// (rustils' D1/D9 terminal handoff, forced by this exact call site —
/// see `baileyrd/rustils#43`), which ignores `SIGTTOU` itself on every
/// call. Other Unix targets keep the raw `tcsetpgrp` call: rustils has no
/// macOS/BSD backend yet, and `init`'s own `SIG_IGN` on `SIGTTOU` (in
/// `JOB_SIGNALS`) already covers the same precondition there.
#[cfg(target_os = "linux")]
fn tcsetpgrp_impl(pgid: pid_t) {
    use platform::term::JobControlTerminal;
    let _ = platform_linux::LinuxTerminal::new().give_terminal(pgid as u32);
}

#[cfg(not(target_os = "linux"))]
fn tcsetpgrp_impl(pgid: pid_t) {
    unsafe {
        // SIGTTOU is ignored in the shell, so this never stops us.
        libc::tcsetpgrp(libc::STDIN_FILENO, pgid);
    }
}

fn give_terminal(pgid: pid_t) {
    if job_control_enabled() {
        tcsetpgrp_impl(pgid);
    }
}

fn reclaim_terminal() {
    let shell_pgid = STATE.with(|s| s.borrow().shell_pgid);
    give_terminal(shell_pgid);
}

pub(crate) fn job_control_enabled() -> bool {
    STATE.with(|s| s.borrow().job_control)
}

/// Clear the in-memory job table for a fresh evaluation (see
/// [`crate::reset_state`]). The terminal / job-control configuration
/// (`shell_pgid`, `job_control`) established by [`init`] is host setup, not
/// per-run shell state, so it is left intact.
pub(crate) fn reset() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.jobs.clear();
        s.next_id = 0;
    });
}

fn state_label(state: JobState) -> &'static str {
    match state {
        JobState::Running => "Running",
        JobState::Stopped => "Stopped",
        JobState::Done => "Done",
    }
}

fn exit_code(status: c_int) -> i32 {
    if wifexited(status) {
        libc::WEXITSTATUS(status)
    } else if wifsignaled(status) {
        128 + libc::WTERMSIG(status) as i32
    } else {
        0
    }
}

// libc exposes the wait-status macros as functions; thin wrappers keep the call
// sites readable and centralise the `c_int` plumbing.
fn wifexited(status: c_int) -> bool {
    libc::WIFEXITED(status)
}
fn wifsignaled(status: c_int) -> bool {
    libc::WIFSIGNALED(status)
}
fn wifstopped(status: c_int) -> bool {
    libc::WIFSTOPPED(status)
}
fn wifcontinued(status: c_int) -> bool {
    libc::WIFCONTINUED(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::{Duration, Instant};

    /// H2 regression: a background child must be reaped even when job control is
    /// **off** (the embedded-PTY mode, RFC 0002). Before the fix
    /// `reap_background` early-returned whenever `job_control` was false, so `&`
    /// jobs spawned by an embedded shell leaked as zombies.
    ///
    /// Safe under the parallel test harness because no other test in this crate
    /// spawns an external process (the common commands are builtins), so this is
    /// the only reapable child in the process.
    #[test]
    fn reap_background_collects_children_when_job_control_off() {
        reset();
        // Not embedded/interactive here, so job control is off — the exact
        // configuration the bug mishandled.
        assert!(!job_control_enabled());

        // Spawn a real, fast-exiting child and register it as a background job
        // the way `run_background` does (track the raw pid; the std handle's
        // Drop neither waits nor kills on Unix, so reaping is the table's job).
        let child = Command::new("true").spawn().expect("spawn `true`");
        let pid = child.id() as pid_t;
        drop(child);
        add_job(pid, &[pid], "true", JobState::Running);

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            reap_background();
            if STATE.with(|s| s.borrow().jobs.is_empty()) {
                break; // reaped, reported Done, and pruned
            }
            assert!(
                Instant::now() < deadline,
                "background job was never reaped with job control off"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        reset();
    }

    /// L3 regression: a fresh evaluation must not inherit a prior run's job
    /// table. `reset` clears the in-memory jobs and the id counter.
    #[test]
    fn reset_clears_job_table() {
        reset();
        add_job(4242, &[4242], "sleep 99", JobState::Running);
        assert_eq!(STATE.with(|s| s.borrow().jobs.len()), 1);

        reset();
        assert!(STATE.with(|s| s.borrow().jobs.is_empty()));
        assert_eq!(STATE.with(|s| s.borrow().next_id), 0);
    }
}
