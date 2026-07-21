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
//!
//! Spawning, signalling, and waiting on a pipeline's members go through
//! [`JobHandle`], which has two implementations: [`LinuxJobHandle`], backed
//! by rustils' `platform::process`/`platform::fs` (D1/D9/D10 —
//! `baileyrd/rustils#43-46,#51`, forced by this exact module — see
//! `baileyrd/nexus#454`), and [`RawPidJobHandle`], the original hand-rolled
//! `setpgid`/`killpg`/`waitpid` implementation kept for every other Unix
//! target: rustils has no macOS/BSD process backend yet (only a net-only
//! one, `baileyrd/rustils#48`), so this repo's real `release-macos.yml`
//! build still needs a working path. Everything above the `JobHandle` layer
//! (the job table, `jobs`/`fg`/`bg`/`kill` builtins) is platform-agnostic.

use std::cell::RefCell;

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
    handle: Box<dyn JobHandle>,
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

/// A no-op signal handler, installed instead of literal `SIG_IGN` on Linux
/// (see [`init`]): unlike `SIG_IGN`, a *caught* signal's disposition resets
/// to `SIG_DFL` across `exec` automatically, which is what lets a child
/// spawned through `platform::process::Spawner` (no `pre_exec` hook to reset
/// dispositions post-fork) still respond normally to Ctrl-C/Ctrl-Z. Verified
/// empirically with a small `posix_spawn` test before relying on it.
#[cfg(target_os = "linux")]
extern "C" fn ignore_job_signal(_sig: c_int) {}

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

    #[cfg(target_os = "linux")]
    unsafe {
        // Neutralize whatever SIGCHLD disposition this process inherited from
        // its own parent (some job-controlling hosts set SIG_IGN) with a real
        // handler rather than SIG_IGN, for the same exec-reset reason as
        // `ignore_job_signal`'s doc comment — the no-pre_exec-hook equivalent
        // of the raw-pid path's per-spawn SIGCHLD reset below. The shell
        // itself never relies on SIGCHLD (it reaps via explicit waitpid/
        // wait_job calls throughout), so a no-op handler is behaviorally
        // inert here.
        libc::signal(libc::SIGCHLD, ignore_job_signal as *const () as libc::sighandler_t);
    }

    if enable {
        unsafe {
            #[cfg(target_os = "linux")]
            for &sig in &JOB_SIGNALS {
                libc::signal(sig, ignore_job_signal as *const () as libc::sighandler_t);
            }
            #[cfg(not(target_os = "linux"))]
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

// ---- job handle: spawn, signal, wait — the platform-specific layer ----------

/// How a spawned pipeline's members are tracked, signalled, and waited on.
/// Everything above this trait (the job table, builtins) is platform-agnostic.
trait JobHandle {
    /// The process-group id every member shares: the `tcsetpgrp` target and
    /// the `jobs`/`bg` display value.
    fn pgid(&self) -> pid_t;
    /// Send a raw signal number (`kill_cmd`'s `-SIG` argument, or `SIGCONT`
    /// for `fg`/`bg`) to every member.
    fn signal_group(&self, sig: c_int);
    /// Block until a member exits, is killed, or stops, reaping members that
    /// terminate.
    fn wait(&mut self) -> Wait;
    /// Non-blocking: `Some(new_state)` if this job's state changed (stopped,
    /// resumed, or every member has now exited) since the last call; `None`
    /// if nothing changed. Terminal members are reaped as a side effect
    /// either way. Used by [`reap_background`].
    fn poll(&mut self) -> Option<JobState>;
}

enum Wait {
    Done(i32),
    Stopped(i32),
}

// ---- Linux: rustils-backed job handle ----------------------------------------

#[cfg(target_os = "linux")]
struct LinuxJobHandle {
    children: Vec<Box<dyn platform::process::Child>>,
    live: usize,
}

#[cfg(target_os = "linux")]
impl JobHandle for LinuxJobHandle {
    fn pgid(&self) -> pid_t {
        self.children[0].id() as pid_t
    }

    fn signal_group(&self, sig: c_int) {
        match signal_from_raw(sig) {
            Some(s) => {
                let _ = self.children[0].kill_tree(s);
            }
            // Not one of rustils' portable Signal variants (an arbitrary
            // numeric signal `kill -N` allows but Signal doesn't cover) —
            // fall back to a direct killpg on the group's own pid, exactly
            // as the raw-pid path always does.
            None => unsafe {
                libc::killpg(self.pgid(), sig);
            },
        }
    }

    fn wait(&mut self) -> Wait {
        use platform::process::ExitStatus;

        let count = self.children.len();
        let last = count - 1;
        let mut done = vec![false; count];
        let mut live = self.live;
        let mut last_code = 0;

        loop {
            for (i, child) in self.children.iter_mut().enumerate() {
                if done[i] {
                    continue;
                }
                // A single stage can block directly; a pipeline needs to
                // poll every stage since there's no "wait for any of these"
                // primitive on `Child`.
                let polled = if count == 1 {
                    child.wait_job().map(Some)
                } else {
                    child.try_wait_job()
                };
                match polled {
                    Ok(Some(ExitStatus::Stopped(sig))) => return Wait::Stopped(128 + sig),
                    // Not terminal — keep waiting on this child.
                    Ok(Some(ExitStatus::Continued)) | Ok(None) => {}
                    Ok(Some(status)) => {
                        done[i] = true;
                        live -= 1;
                        if i == last {
                            last_code = pexit_code(status);
                        }
                    }
                    Err(_) => {
                        done[i] = true;
                        live -= 1;
                    }
                }
            }
            if live == 0 {
                self.live = 0;
                return Wait::Done(last_code);
            }
            if count > 1 {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }

    fn poll(&mut self) -> Option<JobState> {
        use platform::process::ExitStatus;

        let mut changed = None;
        for child in &mut self.children {
            if let Ok(Some(status)) = child.try_wait_job() {
                match status {
                    ExitStatus::Stopped(_) => changed = Some(JobState::Stopped),
                    ExitStatus::Continued => changed = Some(JobState::Running),
                    _ => {
                        self.live = self.live.saturating_sub(1);
                        if self.live == 0 {
                            changed = Some(JobState::Done);
                        }
                    }
                }
            }
        }
        changed
    }
}

#[cfg(target_os = "linux")]
fn pexit_code(status: platform::process::ExitStatus) -> i32 {
    use platform::process::ExitStatus;
    match status {
        ExitStatus::Code(c) => c,
        ExitStatus::Signaled(sig) => 128 + sig,
        // Never reached: callers only pass a status here once it's terminal.
        ExitStatus::Stopped(_) | ExitStatus::Continued => 0,
    }
}

#[cfg(target_os = "linux")]
fn signal_from_raw(sig: c_int) -> Option<platform::process::Signal> {
    use platform::process::Signal;
    Some(match sig {
        libc::SIGTERM => Signal::Term,
        libc::SIGINT => Signal::Int,
        libc::SIGHUP => Signal::Hup,
        libc::SIGQUIT => Signal::Quit,
        libc::SIGKILL => Signal::Kill,
        libc::SIGSTOP => Signal::Stop,
        libc::SIGCONT => Signal::Cont,
        _ => return None,
    })
}

/// Spawn every stage of a pipeline into a single new process group, returning
/// the [`JobHandle`] that owns and controls them. Stage 0 leads a fresh group
/// (`GroupSpec::NewGroup`); stage 1..n join it (`GroupSpec::JoinGroup`) — D1's
/// pipeline shape, race-free at spawn (`baileyrd/rustils#44`).
#[cfg(target_os = "linux")]
fn spawn_pipeline(pipeline: &Pipeline) -> Result<Box<dyn JobHandle>, String> {
    use platform::process::{GroupSpec, Spawner};

    let n = pipeline.commands.len();
    let mut children: Vec<Box<dyn platform::process::Child>> = Vec::with_capacity(n);
    let mut pgid: u32 = 0;
    let mut prev_stdout: Option<Box<dyn platform::fs::File>> = None;

    for (i, cmd) in pipeline.commands.iter().enumerate() {
        let is_last = i == n - 1;
        let group = if i == 0 {
            GroupSpec::NewGroup
        } else {
            GroupSpec::JoinGroup(pgid)
        };
        let command = build_pcommand(cmd, prev_stdout.take(), is_last, group)?;

        let mut child = platform_linux::LinuxSpawner
            .spawn(&command)
            .map_err(|e| format!("{}: {e}", cmd.argv[0]))?;
        if i == 0 {
            pgid = child.id();
        }
        feed_heredoc_p(&mut *child, cmd);

        if !is_last {
            prev_stdout = child.take_stdout();
        }
        children.push(child);
    }

    let live = children.len();
    Ok(Box::new(LinuxJobHandle { children, live }))
}

/// Where one descriptor is routed, mirroring `exec::Sink` but over
/// `platform::fs::File` (`baileyrd/rustils#51`) instead of `std::fs::File`.
#[cfg(target_os = "linux")]
enum PSink {
    Inherit,
    Pipe,
    File(Box<dyn platform::fs::File>),
}

#[cfg(target_os = "linux")]
impl PSink {
    fn into_stdio(self) -> platform::process::Stdio {
        match self {
            PSink::Inherit => platform::process::Stdio::Inherit,
            PSink::Pipe => platform::process::Stdio::Pipe,
            PSink::File(f) => platform::process::Stdio::File(f),
        }
    }
}

#[cfg(target_os = "linux")]
fn clone_psink(target: u32, stdout: &PSink, stderr: &PSink) -> Result<PSink, String> {
    let src = if target == 2 { stderr } else { stdout };
    Ok(match src {
        PSink::Inherit | PSink::Pipe => PSink::Inherit,
        PSink::File(f) => PSink::File(f.try_clone().map_err(|e| e.to_string())?),
    })
}

#[cfg(target_os = "linux")]
fn open_pfile_read(file: &str) -> Result<Box<dyn platform::fs::File>, String> {
    let f = std::fs::File::open(file).map_err(|e| format!("{file}: {e}"))?;
    wrap_pfile(f)
}

#[cfg(target_os = "linux")]
fn open_pfile_write(file: &str, append: bool) -> Result<Box<dyn platform::fs::File>, String> {
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .append(append)
        .truncate(!append)
        .open(file)
        .map_err(|e| format!("{file}: {e}"))?;
    wrap_pfile(f)
}

/// Wrap a plain `std::fs::File` (already-tested open/redirect resolution) as
/// a `platform::fs::File` — `LinuxFile::from(OwnedFd)` (`baileyrd/rustils#43`
/// era API), sidestepping rustils' capability-based `Dir` open path entirely.
#[cfg(target_os = "linux")]
fn wrap_pfile(f: std::fs::File) -> Result<Box<dyn platform::fs::File>, String> {
    let fd: std::os::fd::OwnedFd = f.into();
    Ok(Box::new(platform_linux::LinuxFile::from(fd)))
}

/// Build the `platform::process::Command` for one pipeline stage — the
/// rustils-backed sibling of `exec::build_stage`, kept separate rather than
/// unified with it: `build_stage` is also the non-Unix/non-job-control plain
/// runner's spawn path (Windows included), which stays on `std::process`
/// entirely untouched by this conversion.
#[cfg(target_os = "linux")]
fn build_pcommand(
    cmd: &crate::exec::Command,
    stdin_src: Option<Box<dyn platform::fs::File>>,
    is_last: bool,
    group: platform::process::GroupSpec,
) -> Result<platform::process::Command, String> {
    use crate::exec::{RedirMode, Redirect};
    use platform::process::EnvSpec;
    use std::ffi::OsString;

    let program = cmd
        .argv
        .first()
        .ok_or_else(|| "empty command".to_string())?;
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;

    // Ambient process environment first (matches `std::process::Command`'s
    // default inherit-unless-cleared behavior, which `build_stage` relies
    // on), then exported shell variables, then this command's own
    // assignments — each layer overriding the last.
    let mut env: std::collections::BTreeMap<OsString, OsString> = std::env::vars_os().collect();
    for (k, v) in crate::vars::exported() {
        env.insert(OsString::from(k), OsString::from(v));
    }
    for (k, v) in &cmd.assignments {
        env.insert(OsString::from(k.as_str()), OsString::from(v.as_str()));
    }

    let mut stdin_sink = stdin_src.map(PSink::File).unwrap_or(PSink::Inherit);
    let mut stdout_sink = if is_last { PSink::Inherit } else { PSink::Pipe };
    let mut stderr_sink = PSink::Inherit;

    for r in &cmd.redirects {
        match r {
            Redirect::File { fd, file, mode } => match mode {
                RedirMode::Read => {
                    if *fd == 0 {
                        stdin_sink = PSink::File(open_pfile_read(file)?);
                    }
                }
                RedirMode::Write | RedirMode::Append => {
                    let f = open_pfile_write(file, *mode == RedirMode::Append)?;
                    match fd {
                        0 => stdin_sink = PSink::File(f),
                        2 => stderr_sink = PSink::File(f),
                        _ => stdout_sink = PSink::File(f),
                    }
                }
            },
            Redirect::Both { file, append } => {
                let f = open_pfile_write(file, *append)?;
                let g = f.try_clone().map_err(|e| e.to_string())?;
                stdout_sink = PSink::File(f);
                stderr_sink = PSink::File(g);
            }
            Redirect::Dup { fd, target } => {
                let cloned = clone_psink(*target, &stdout_sink, &stderr_sink)?;
                match fd {
                    2 => stderr_sink = cloned,
                    _ => stdout_sink = cloned,
                }
            }
        }
    }

    // A here-document feeds stdin from a pipe we write after spawn.
    if cmd.heredoc.is_some() {
        stdin_sink = PSink::Pipe;
    }

    Ok(platform::process::Command {
        program: OsString::from(program.as_str()),
        argv: cmd.argv[1..]
            .iter()
            .map(|a| OsString::from(a.as_str()))
            .collect(),
        cwd: cwd.into_os_string(),
        env: EnvSpec::Explicit(env),
        stdin: stdin_sink.into_stdio(),
        stdout: stdout_sink.into_stdio(),
        stderr: stderr_sink.into_stdio(),
        group,
    })
}

/// `platform::fs::File`'s object-safe `Box<dyn File>` carries no `Send`
/// bound of its own (RFC v2 §5.1 keeps object-safe traits here minimal), but
/// every real backend's concrete file type is just an owned fd/handle —
/// exactly as `Send` as `std::fs::File`. Asserted here rather than added to
/// the trait, matching `nexus_terminal::job_object::JobObject`'s own
/// `unsafe impl Send`/`Sync` for the same "it's just a handle" reason.
#[cfg(target_os = "linux")]
struct SendFile(Box<dyn platform::fs::File>);
#[cfg(target_os = "linux")]
unsafe impl Send for SendFile {}

/// Write a command's here-document body to its stdin on a background thread —
/// the rustils-`Child` sibling of `exec::feed_heredoc`, over `File::write`'s
/// raw (non-`std::io::Write`) contract.
#[cfg(target_os = "linux")]
fn feed_heredoc_p(child: &mut dyn platform::process::Child, cmd: &crate::exec::Command) {
    if let Some(body) = &cmd.heredoc
        && let Some(stdin) = child.take_stdin()
    {
        let stdin = SendFile(stdin);
        let body = body.clone();
        std::thread::spawn(move || {
            // Forces the closure to capture the whole `SendFile` (whose
            // `Send` is asserted) rather than just its `.0` field
            // (`Box<dyn File>`, not `Send`) — Rust 2021+'s disjoint
            // closure captures would otherwise capture the field alone.
            let mut stdin = stdin;
            let bytes = body.as_bytes();
            let mut written = 0;
            while written < bytes.len() {
                match stdin.0.write(&bytes[written..]) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => written += n,
                }
            }
        });
    }
}

// ---- non-Linux Unix: the original raw-pid job handle -------------------------

#[cfg(not(target_os = "linux"))]
struct RawPidJobHandle {
    pgid: pid_t,
    pids: Vec<pid_t>,
    live: usize,
}

#[cfg(not(target_os = "linux"))]
impl JobHandle for RawPidJobHandle {
    fn pgid(&self) -> pid_t {
        self.pgid
    }

    fn signal_group(&self, sig: c_int) {
        unsafe {
            libc::killpg(self.pgid, sig);
        }
    }

    fn wait(&mut self) -> Wait {
        let last = *self.pids.last().expect("pipeline has at least one stage");
        let mut last_code = 0;

        while self.live > 0 {
            let mut status: c_int = 0;
            let wpid = unsafe { libc::waitpid(-self.pgid, &mut status, libc::WUNTRACED) };
            if wpid <= 0 {
                break; // -1 (ECHILD) or 0: nothing left to wait for
            }
            if wifstopped(status) {
                return Wait::Stopped(128 + libc::WSTOPSIG(status) as i32);
            }
            // Exited or killed by a signal.
            self.live -= 1;
            if wpid == last {
                last_code = exit_code(status);
            }
        }

        Wait::Done(last_code)
    }

    fn poll(&mut self) -> Option<JobState> {
        let mut changed = None;
        loop {
            let mut status: c_int = 0;
            let flags = libc::WNOHANG | libc::WUNTRACED | libc::WCONTINUED;
            let wpid = unsafe { libc::waitpid(-self.pgid, &mut status, flags) };
            if wpid <= 0 {
                break; // 0: no change; -1: no children left in this group
            }
            if wifstopped(status) {
                changed = Some(JobState::Stopped);
            } else if wifcontinued(status) {
                changed = Some(JobState::Running);
            } else {
                // Exited or signaled.
                self.live = self.live.saturating_sub(1);
                if self.live == 0 {
                    changed = Some(JobState::Done);
                }
            }
        }
        changed
    }
}

/// Spawn every stage of a pipeline into a single new process group, returning
/// the [`JobHandle`] that owns and controls them. Children reset signal
/// dispositions and join the group before `exec`; the parent also calls
/// `setpgid` to avoid racing terminal hand-off.
#[cfg(not(target_os = "linux"))]
fn spawn_pipeline(pipeline: &Pipeline) -> Result<Box<dyn JobHandle>, String> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;

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

    let live = pids.len();
    Ok(Box::new(RawPidJobHandle { pgid, pids, live }))
}

// libc exposes the wait-status macros as functions; thin wrappers keep the call
// sites readable and centralise the `c_int` plumbing. Only the raw-pid job
// handle needs these — the Linux path gets decoded `ExitStatus` from rustils.
#[cfg(not(target_os = "linux"))]
fn exit_code(status: c_int) -> i32 {
    if wifexited(status) {
        libc::WEXITSTATUS(status)
    } else if wifsignaled(status) {
        128 + libc::WTERMSIG(status) as i32
    } else {
        0
    }
}
#[cfg(not(target_os = "linux"))]
fn wifexited(status: c_int) -> bool {
    libc::WIFEXITED(status)
}
#[cfg(not(target_os = "linux"))]
fn wifsignaled(status: c_int) -> bool {
    libc::WIFSIGNALED(status)
}
#[cfg(not(target_os = "linux"))]
fn wifstopped(status: c_int) -> bool {
    libc::WIFSTOPPED(status)
}
#[cfg(not(target_os = "linux"))]
fn wifcontinued(status: c_int) -> bool {
    libc::WIFCONTINUED(status)
}

// ---- job table + builtins: platform-agnostic ---------------------------------

/// Run a pipeline in the foreground, returning its exit status. If it stops
/// (Ctrl-Z), it is added to the job table and we return `128 + SIGTSTP`.
pub fn run_foreground(pipeline: &Pipeline) -> Result<i32, String> {
    let mut handle = spawn_pipeline(pipeline)?;
    give_terminal(handle.pgid());

    let result = handle.wait();
    reclaim_terminal();

    Ok(match result {
        Wait::Done(code) => code,
        Wait::Stopped(code) => {
            let cmd = crate::exec::pipeline_text(pipeline);
            let id = add_job(handle, &cmd, JobState::Stopped);
            eprintln!("\n[{id}]+  Stopped\t{cmd}");
            code
        }
    })
}

/// Run a pipeline in the background: record it and print `[id] pgid`.
pub fn run_background(pipeline: &Pipeline) -> Result<(), String> {
    let handle = spawn_pipeline(pipeline)?;
    let pgid = handle.pgid();
    let cmd = crate::exec::pipeline_text(pipeline);
    let id = add_job(handle, &cmd, JobState::Running);
    println!("[{id}] {pgid}");
    Ok(())
}

/// Reap finished/stopped/continued background jobs without blocking, reporting
/// state changes. Called once before each prompt.
///
/// This runs **regardless of `job_control`**: a shell embedded in a Nexus-owned
/// PTY (RFC 0002) keeps job control off, but `&` still spawns real children via
/// [`run_background`]. Without reaping them here those children would linger as
/// zombies for the life of the embedded shell. Foreground commands in embedded
/// mode are already waited synchronously (see `exec::run_foreground`), so every
/// job left to poll here is a tracked background job.
pub fn reap_background() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        for job in &mut s.jobs {
            if let Some(new_state) = job.handle.poll() {
                if new_state == JobState::Stopped {
                    job.notified = false;
                }
                job.state = new_state;
            }
        }
    });

    notify_and_prune();
}

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
            let found = spec.parse::<usize>().ok().and_then(|id| {
                STATE.with(|s| {
                    s.borrow()
                        .jobs
                        .iter()
                        .find(|j| j.id == id)
                        .map(|j| j.handle.signal_group(sig))
                })
            });
            if found.is_none() {
                eprintln!("kill: %{spec}: no such job");
                status = 1;
            }
        } else if let Ok(pid) = target.parse::<pid_t>() {
            // A bare pid isn't necessarily one of our own spawned children —
            // could be any process the caller has permission to signal —
            // so this always goes through raw `libc::kill` regardless of
            // platform; there's no `Child` handle to call a typed method on.
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
    give_terminal(job.handle.pgid());
    job.handle.signal_group(libc::SIGCONT);

    let result = job.handle.wait();
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
        job.handle.signal_group(libc::SIGCONT);
        println!("[{}] {} &", job.id, job.cmd);
        0
    })
}

// ---- job table helpers -------------------------------------------------------

fn add_job(handle: Box<dyn JobHandle>, cmd: &str, state: JobState) -> usize {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.next_id += 1;
        let id = s.next_id;
        s.jobs.push(JobEntry {
            id,
            handle,
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
/// macOS/BSD process backend yet, and `init`'s own `SIG_IGN` on `SIGTTOU` (in
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// H2 regression: a background child must be reaped even when job control is
    /// **off** (the embedded-PTY mode, RFC 0002). Before the fix
    /// `reap_background` early-returned whenever `job_control` was false, so `&`
    /// jobs spawned by an embedded shell leaked as zombies.
    ///
    /// Safe under the parallel test harness because no other test in this crate
    /// spawns an external process (the common commands are builtins), so this is
    /// the only reapable child in the process.
    #[cfg(target_os = "linux")]
    #[test]
    fn reap_background_collects_children_when_job_control_off() {
        use platform::process::{EnvSpec, GroupSpec, Spawner, Stdio};

        reset();
        // Not embedded/interactive here, so job control is off — the exact
        // configuration the bug mishandled.
        assert!(!job_control_enabled());

        let command = platform::process::Command {
            program: "true".into(),
            argv: Vec::new(),
            cwd: std::env::current_dir().unwrap().into_os_string(),
            env: EnvSpec::Inherit,
            stdin: Stdio::Inherit,
            stdout: Stdio::Inherit,
            stderr: Stdio::Inherit,
            group: GroupSpec::NewGroup,
        };
        let child = platform_linux::LinuxSpawner
            .spawn(&command)
            .expect("spawn `true`");
        let handle: Box<dyn JobHandle> = Box::new(LinuxJobHandle {
            children: vec![child],
            live: 1,
        });
        add_job(handle, "true", JobState::Running);

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

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn reap_background_collects_children_when_job_control_off() {
        reset();
        assert!(!job_control_enabled());

        // Spawn a real, fast-exiting child and register it as a background job
        // the way `run_background` does (track the raw pid; the std handle's
        // Drop neither waits nor kills on Unix, so reaping is the table's job).
        let child = std::process::Command::new("true")
            .spawn()
            .expect("spawn `true`");
        let pid = child.id() as pid_t;
        drop(child);
        let handle: Box<dyn JobHandle> = Box::new(RawPidJobHandle {
            pgid: pid,
            pids: vec![pid],
            live: 1,
        });
        add_job(handle, "true", JobState::Running);

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
    #[cfg(target_os = "linux")]
    #[test]
    fn reset_clears_job_table() {
        use platform::process::{EnvSpec, GroupSpec, Spawner, Stdio};

        reset();
        let command = platform::process::Command {
            program: "true".into(),
            argv: Vec::new(),
            cwd: std::env::current_dir().unwrap().into_os_string(),
            env: EnvSpec::Inherit,
            stdin: Stdio::Inherit,
            stdout: Stdio::Inherit,
            stderr: Stdio::Inherit,
            group: GroupSpec::NewGroup,
        };
        let child = platform_linux::LinuxSpawner
            .spawn(&command)
            .expect("spawn `true`");
        let handle: Box<dyn JobHandle> = Box::new(LinuxJobHandle {
            children: vec![child],
            live: 1,
        });
        add_job(handle, "sleep 99", JobState::Running);
        assert_eq!(STATE.with(|s| s.borrow().jobs.len()), 1);

        reset();
        assert!(STATE.with(|s| s.borrow().jobs.is_empty()));
        assert_eq!(STATE.with(|s| s.borrow().next_id), 0);
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn reset_clears_job_table() {
        reset();
        let handle: Box<dyn JobHandle> = Box::new(RawPidJobHandle {
            pgid: 4242,
            pids: vec![4242],
            live: 1,
        });
        add_job(handle, "sleep 99", JobState::Running);
        assert_eq!(STATE.with(|s| s.borrow().jobs.len()), 1);

        reset();
        assert!(STATE.with(|s| s.borrow().jobs.is_empty()));
        assert_eq!(STATE.with(|s| s.borrow().next_id), 0);
    }
}
