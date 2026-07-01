//! Execute a parsed command list.
//!
//! A [`CommandList`] is a sequence of jobs separated by `;`/`&`. Each job is an
//! and-or chain of pipelines (`&&`/`||`); a job marked `background` runs without
//! blocking the shell. Every pipeline is expanded (variables, globs, …) *just
//! before it runs*, left to right, so a `cd` takes effect for later pipelines.
//!
//! On Unix, foreground and background pipelines go through [`crate::job`], which
//! adds process groups, terminal control, and stop/`fg`/`bg` handling. On other
//! platforms there is no job control: foreground pipelines run with a plain
//! spawn-and-wait, and `&` is rejected.
//!
//! Within a pipeline, builtins only run in-process when the pipeline is a
//! single command — a builtin in the middle of a pipe (`echo hi | cd`) is a
//! rare case we punt on for now.

use std::fs::{File, OpenOptions};
use std::io::Read;
use std::process::{Child, Command as OsCommand, Stdio};

use crate::builtins;
use crate::parser::{AndOrList, CommandList, Compound, Connector, Job, RawCommand, RawPipeline};

#[derive(Debug, Clone)]
pub struct Command {
    pub argv: Vec<String>,
    pub redirects: Vec<Redirect>,
    /// Leading `NAME=value` assignments. With no `argv` they set shell variables;
    /// otherwise they apply to this command's environment only.
    pub assignments: Vec<(String, String)>,
    /// A here-document body (already expanded) to feed on stdin, if any.
    pub heredoc: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Redirect {
    /// `[fd]< file` / `[fd]> file` / `[fd]>> file`.
    File {
        fd: u32,
        file: String,
        mode: RedirMode,
    },
    /// `&> file` / `&>> file`.
    Both { file: String, append: bool },
    /// `fd>&target` (e.g. `2>&1`).
    Dup { fd: u32, target: u32 },
}

pub use crate::parser::RedirMode;

#[derive(Debug, Clone)]
pub struct Pipeline {
    pub commands: Vec<Command>,
}

/// Run a whole command line, returning the exit status of the last foreground
/// job that ran. A `break`/`continue` that escapes all loops is discarded here.
pub fn run_list(list: &CommandList) -> Result<i32, String> {
    let status = exec_list(list)?;
    // Any break/continue/return that escaped to the top level is discarded.
    crate::vars::set_loop_ctl(None);
    crate::vars::set_returning(None);
    Ok(status)
}

/// Run a list, stopping early if `break`/`continue`/`return` becomes pending —
/// used for both the top level and the bodies of compound commands.
fn exec_list(list: &CommandList) -> Result<i32, String> {
    let mut status = 0;
    for job in &list.jobs {
        status = run_job(job)?;
        if crate::vars::flow_pending() {
            break;
        }
    }
    Ok(status)
}

fn run_job(job: &Job) -> Result<i32, String> {
    if job.background {
        // Backgrounding an `&&`/`||` list would need a subshell; we support the
        // common case of a single (possibly piped) command.
        if !job.list.rest.is_empty() {
            return Err("background '&&'/'||' lists are not supported".into());
        }
        let pipeline = crate::expand::expand(&job.list.first)?;
        run_background(&pipeline)?;
        crate::vars::set_last_status(0);
        Ok(0)
    } else {
        run_andor(&job.list)
    }
}

fn run_andor(list: &AndOrList) -> Result<i32, String> {
    // Update `$?` after every pipeline, so a later one in the same line can read
    // it (e.g. `false || echo $?`).
    let mut status = run_pipeline_node(&list.first)?;
    crate::vars::set_last_status(status);
    if crate::vars::flow_pending() {
        return Ok(status);
    }
    for (connector, raw) in &list.rest {
        if should_run(*connector, status) {
            status = run_pipeline_node(raw)?;
            crate::vars::set_last_status(status);
            if crate::vars::flow_pending() {
                break;
            }
        }
    }
    Ok(status)
}

/// A pipeline that is a single compound command (`if`/`while`/`for`) is run
/// directly; everything else goes through the simple-command path.
fn run_pipeline_node(raw: &RawPipeline) -> Result<i32, String> {
    if let [RawCommand::Compound(compound)] = raw.commands.as_slice() {
        return run_compound(compound);
    }
    run_foreground(raw)
}

fn run_compound(compound: &Compound) -> Result<i32, String> {
    match compound {
        Compound::If {
            branches,
            else_body,
        } => {
            for (cond, body) in branches {
                if exec_list(cond)? == 0 {
                    return exec_list(body);
                }
            }
            match else_body {
                Some(body) => exec_list(body),
                None => Ok(0),
            }
        }
        Compound::Loop { until, cond, body } => {
            let mut status = 0;
            loop {
                let met = exec_list(cond)? == 0;
                if met == *until {
                    break; // while: stop when not met; until: stop when met
                }
                status = exec_list(body)?;
                if loop_step()? {
                    break;
                }
            }
            Ok(status)
        }
        Compound::For { var, words, body } => {
            let mut status = 0;
            for value in crate::expand::expand_words(words)? {
                crate::vars::set(var, &value);
                status = exec_list(body)?;
                if loop_step()? {
                    break;
                }
            }
            Ok(status)
        }
        Compound::Case { word, items } => {
            let subject = crate::expand::expand_to_string(word)?;
            for (patterns, body) in items {
                for pat in patterns {
                    if crate::glob::match_component(&crate::expand::expand_pattern(pat)?, &subject)
                    {
                        return exec_list(body);
                    }
                }
            }
            Ok(0)
        }
        Compound::Group(list) => exec_list(list),
        Compound::Subshell(list) => {
            // A real subshell forks; we approximate by isolating the state that
            // commands usually mutate — the working directory and variables — so
            // `(cd x; …)` and `(VAR=…; …)` don't leak out. (`exit` inside still
            // exits the whole shell — a known limitation of not forking.)
            let saved_cwd = std::env::current_dir().ok();
            let saved_vars = crate::vars::snapshot();

            let result = exec_list(list);

            if let Some(dir) = saved_cwd {
                let _ = std::env::set_current_dir(dir);
            }
            crate::vars::restore(saved_vars);
            result
        }
        Compound::FuncDef { name, body } => {
            crate::func::define(name, body.clone());
            Ok(0)
        }
    }
}

/// Run a defined function: swap in the call's arguments as `$1`…, run the body
/// (a `return` ends it), then restore the previous positional parameters.
fn call_function(argv: &[String]) -> Result<i32, String> {
    let body = crate::func::get(&argv[0]).expect("function is defined");

    let name0 = crate::vars::arg(0).unwrap_or_else(|| "rush".to_string());
    let saved = crate::vars::args();
    crate::vars::set_args(name0.clone(), argv[1..].to_vec());

    let result = exec_list(&body);

    let returned = crate::vars::returning();
    crate::vars::set_returning(None);
    crate::vars::set_args(name0, saved);

    Ok(returned.unwrap_or(result?))
}

/// After running a loop body, consume one level of any pending `break`/
/// `continue`. Returns `true` if this loop should stop iterating.
fn loop_step() -> Result<bool, String> {
    use crate::vars::LoopCtl;
    // A pending `exit` unwinds straight out of every loop (left for `lib::eval`
    // / the REPL to turn into the process exit code).
    if crate::vars::exit_requested().is_some() {
        return Ok(true);
    }
    // A pending `return` unwinds straight through the loop (left for the
    // enclosing function call to consume).
    if crate::vars::returning().is_some() {
        return Ok(true);
    }
    match crate::vars::loop_ctl() {
        None => Ok(false),
        Some(LoopCtl::Continue(1)) => {
            crate::vars::set_loop_ctl(None);
            Ok(false) // keep looping
        }
        Some(LoopCtl::Break(1)) => {
            crate::vars::set_loop_ctl(None);
            Ok(true)
        }
        // `break N` / `continue N` for an outer loop: drop a level and stop this
        // one, leaving the request pending for the enclosing loop to handle.
        Some(LoopCtl::Break(n)) => {
            crate::vars::set_loop_ctl(Some(LoopCtl::Break(n - 1)));
            Ok(true)
        }
        Some(LoopCtl::Continue(n)) => {
            crate::vars::set_loop_ctl(Some(LoopCtl::Continue(n - 1)));
            Ok(true)
        }
    }
}

fn should_run(connector: Connector, prev_status: i32) -> bool {
    match connector {
        Connector::And => prev_status == 0,
        Connector::Or => prev_status != 0,
    }
}

/// A single command that is only `NAME=value` assignments (no program word):
/// `FOO=bar`. These set shell variables rather than spawning anything.
fn assignment_only(pipeline: &Pipeline) -> bool {
    pipeline.commands.len() == 1
        && pipeline.commands[0].argv.is_empty()
        && !pipeline.commands[0].assignments.is_empty()
}

fn apply_assignments(pipeline: &Pipeline) {
    for (name, value) in &pipeline.commands[0].assignments {
        crate::vars::set(name, value);
    }
}

/// Expand and run a single pipeline in the foreground.
fn run_foreground(raw: &RawPipeline) -> Result<i32, String> {
    let pipeline = crate::expand::expand(raw)?;

    if assignment_only(&pipeline) {
        apply_assignments(&pipeline);
        return Ok(0);
    }

    if pipeline.commands.len() == 1 {
        let argv = &pipeline.commands[0].argv;
        // A defined function shadows external commands (but not builtins).
        if argv.first().is_some_and(|name| crate::func::exists(name)) {
            return call_function(argv);
        }
        if let Some(code) = builtins::try_run(argv) {
            return Ok(code);
        }
    }

    #[cfg(unix)]
    {
        // With job control on (interactive, non-embedded), go through the job
        // runner for process groups + terminal hand-off. When it's off — a
        // non-tty shell, or one embedded in a Nexus-owned PTY — run children in
        // the shell's own process group via the plain spawn-and-wait path, so
        // they share the controlling terminal instead of being stopped on it.
        if crate::job::job_control_enabled() {
            crate::job::run_foreground(&pipeline)
        } else {
            run(&pipeline, false).map(|(status, _)| status)
        }
    }
    #[cfg(not(unix))]
    {
        run(&pipeline, false).map(|(status, _)| status)
    }
}

/// Run an already-expanded pipeline in the background. Unix only.
#[cfg(unix)]
fn run_background(pipeline: &Pipeline) -> Result<(), String> {
    crate::job::run_background(pipeline)
}
#[cfg(not(unix))]
fn run_background(_pipeline: &Pipeline) -> Result<(), String> {
    Err("background jobs are not supported on this platform".into())
}

/// Run a command list and return its stdout — the engine behind `$(...)`.
/// Substitutions are synchronous: every job runs in the foreground with a plain
/// spawn-and-wait (no job control), and the `&` background marker is ignored.
pub fn capture_list(list: &CommandList) -> Result<String, String> {
    let mut out = String::new();
    for job in &list.jobs {
        capture_andor(&job.list, &mut out)?;
    }
    Ok(out)
}

fn capture_andor(list: &AndOrList, out: &mut String) -> Result<i32, String> {
    let mut status = capture_pipeline(&list.first, out)?;
    for (connector, raw) in &list.rest {
        if should_run(*connector, status) {
            status = capture_pipeline(raw, out)?;
        }
    }
    Ok(status)
}

fn capture_pipeline(raw: &RawPipeline, out: &mut String) -> Result<i32, String> {
    let pipeline = crate::expand::expand(raw)?;
    if assignment_only(&pipeline) {
        apply_assignments(&pipeline);
        return Ok(0);
    }
    let (status, captured) = run(&pipeline, true)?;
    out.push_str(&captured);
    Ok(status)
}

/// Plain spawn-and-wait runner: used for capture, and as the foreground runner
/// on non-Unix platforms. Returns `(exit status, captured stdout)`; the string
/// is empty unless `capture` is set.
fn run(pipeline: &Pipeline, capture: bool) -> Result<(i32, String), String> {
    let n = pipeline.commands.len();
    let mut children: Vec<Child> = Vec::with_capacity(n);
    // Stdin for the next stage: the read end of the previous stage's pipe.
    let mut prev_stdout: Option<Stdio> = None;
    let mut captured = String::new();

    for (i, cmd) in pipeline.commands.iter().enumerate() {
        let is_last = i == n - 1;
        let mut command = build_stage(cmd, prev_stdout.take(), is_last, capture)?;

        let mut child = command
            .spawn()
            .map_err(|e| format!("{}: {e}", cmd.argv[0]))?;
        feed_heredoc(&mut child, cmd);

        if !is_last {
            prev_stdout = child.stdout.take().map(Stdio::from);
        } else if capture && let Some(mut out) = child.stdout.take() {
            out.read_to_string(&mut captured)
                .map_err(|e| e.to_string())?;
        }
        children.push(child);
    }

    let mut status = 0;
    for (i, mut child) in children.into_iter().enumerate() {
        let exit = child.wait().map_err(|e| e.to_string())?;
        if i == n - 1 {
            status = exit.code().unwrap_or(1);
        }
    }

    Ok((status, captured))
}

/// Build the `std::process::Command` for one pipeline stage: program, args, and
/// stdio. An explicit `<`/`>`/`>>` redirect wins over pipe wiring; otherwise a
/// non-final stage (or any stage when capturing) gets a piped stdout. Shared by
/// the plain runner and the Unix job runner.
pub(crate) fn build_stage(
    cmd: &Command,
    stdin_src: Option<Stdio>,
    is_last: bool,
    capture: bool,
) -> Result<OsCommand, String> {
    let program = cmd
        .argv
        .first()
        .ok_or_else(|| "empty command".to_string())?;
    let mut command = OsCommand::new(program);
    command.args(&cmd.argv[1..]);

    // Seed the environment: exported shell variables first, then this command's
    // own `NAME=value` prefixes (which override).
    command.envs(crate::vars::exported());
    command.envs(cmd.assignments.iter().map(|(k, v)| (k, v)));

    // Resolve the three standard descriptors. fd1 defaults to a pipe when this
    // stage feeds another (or is being captured); the redirects below override
    // in source order, so `> f 2>&1` sends both to `f`.
    let mut stdin_sink: Option<Stdio> = stdin_src;
    let mut stdout_sink = if !is_last || capture {
        Sink::Pipe
    } else {
        Sink::Inherit
    };
    let mut stderr_sink = Sink::Inherit;

    for r in &cmd.redirects {
        match r {
            Redirect::File { fd, file, mode } => match mode {
                RedirMode::Read => {
                    let f = File::open(file).map_err(|e| format!("{file}: {e}"))?;
                    if *fd == 0 {
                        stdin_sink = Some(Stdio::from(f));
                    }
                }
                RedirMode::Write | RedirMode::Append => {
                    let f = open_write(file, *mode == RedirMode::Append)?;
                    match fd {
                        0 => stdin_sink = Some(Stdio::from(f)),
                        2 => stderr_sink = Sink::File(f),
                        _ => stdout_sink = Sink::File(f),
                    }
                }
            },
            Redirect::Both { file, append } => {
                let f = open_write(file, *append)?;
                let g = f.try_clone().map_err(|e| e.to_string())?;
                stdout_sink = Sink::File(f);
                stderr_sink = Sink::File(g);
            }
            Redirect::Dup { fd, target } => {
                let cloned = clone_sink(*target, &stdout_sink, &stderr_sink)?;
                match fd {
                    2 => stderr_sink = cloned,
                    _ => stdout_sink = cloned,
                }
            }
        }
    }

    // A here-document feeds stdin from a pipe we write after spawn.
    if cmd.heredoc.is_some() {
        stdin_sink = Some(Stdio::piped());
    }

    if let Some(s) = stdin_sink {
        command.stdin(s);
    }
    if let Some(s) = stdout_sink.into_stdio()? {
        command.stdout(s);
    }
    if let Some(s) = stderr_sink.into_stdio()? {
        command.stderr(s);
    }

    Ok(command)
}

/// Where one descriptor is routed. Files are kept as handles so `2>&1` can
/// `try_clone` them.
enum Sink {
    Inherit,
    Pipe,
    File(File),
}

impl Sink {
    /// `None` means "leave inherited".
    fn into_stdio(self) -> Result<Option<Stdio>, String> {
        Ok(match self {
            Sink::Inherit => None,
            Sink::Pipe => Some(Stdio::piped()),
            Sink::File(f) => Some(Stdio::from(f)),
        })
    }
}

/// Clone the sink currently bound to `target` (for `fd>&target`). A pipe can't
/// be shared with `std` before spawn, so duping a piped fd falls back to inherit.
fn clone_sink(target: u32, stdout: &Sink, stderr: &Sink) -> Result<Sink, String> {
    let src = if target == 2 { stderr } else { stdout };
    Ok(match src {
        Sink::Inherit | Sink::Pipe => Sink::Inherit,
        Sink::File(f) => Sink::File(f.try_clone().map_err(|e| e.to_string())?),
    })
}

fn open_write(file: &str, append: bool) -> Result<File, String> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .append(append)
        .truncate(!append)
        .open(file)
        .map_err(|e| format!("{file}: {e}"))
}

/// Write a command's here-document body to its stdin on a background thread, so
/// a large body can't deadlock against a child that hasn't started reading.
pub(crate) fn feed_heredoc(child: &mut Child, cmd: &Command) {
    if let Some(body) = &cmd.heredoc
        && let Some(mut stdin) = child.stdin.take()
    {
        let body = body.clone();
        std::thread::spawn(move || {
            use std::io::Write;
            let _ = stdin.write_all(body.as_bytes());
        });
    }
}

/// A human-readable rendering of a pipeline, for the `jobs` listing. Only the
/// Unix job runner uses it.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn pipeline_text(pipeline: &Pipeline) -> String {
    pipeline
        .commands
        .iter()
        .map(|c| c.argv.join(" "))
        .collect::<Vec<_>>()
        .join(" | ")
}
