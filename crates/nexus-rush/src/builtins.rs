//! Builtins that must run inside the shell process.
//!
//! `cd` is the canonical example: if it ran in a child process, the child's
//! working-directory change would die with the child and the shell would
//! never move. So these are handled before we ever spawn anything.

use std::path::Path;

/// Returns `Some(exit_code)` if `argv` named a builtin (and it ran), or
/// `None` if this isn't a builtin and should be exec'd as an external command.
pub fn try_run(argv: &[String]) -> Option<i32> {
    match argv.first().map(String::as_str)? {
        "cd" => Some(cd(argv)),
        "pwd" => Some(pwd()),
        "echo" => Some(echo(argv)),
        "export" => Some(export(argv)),
        "unset" => Some(unset(argv)),
        "test" => Some(test_dispatch(argv, false)),
        "[" => Some(test_dispatch(argv, true)),
        "break" => Some(loop_ctl(argv, true)),
        "continue" => Some(loop_ctl(argv, false)),
        "return" => Some(return_cmd(argv)),
        // POSIX no-op (`:`) and the canonical true/false.
        "true" | ":" => Some(0),
        "false" => Some(1),
        "exit" => Some(exit(argv)), // sets the exit latch; never diverges
        _ => other_builtin(argv),
    }
}

/// `echo [-n] [args...]` — join args with spaces; `-n` suppresses the newline.
/// (No `-e` escape processing, matching the bash default.)
fn echo(argv: &[String]) -> i32 {
    let mut args = &argv[1..];
    let newline = !matches!(args.first(), Some(flag) if flag == "-n");
    if !newline {
        args = &args[1..];
    }

    let line = args.join(" ");
    if newline {
        println!("{line}");
    } else {
        use std::io::Write;
        print!("{line}");
        let _ = std::io::stdout().flush();
    }
    0
}

/// Platform-specific builtins. On Unix this is where `jobs`/`fg`/`bg` live.
#[cfg(unix)]
fn other_builtin(argv: &[String]) -> Option<i32> {
    crate::job::builtin(argv)
}

#[cfg(not(unix))]
fn other_builtin(_argv: &[String]) -> Option<i32> {
    None
}

fn cd(argv: &[String]) -> i32 {
    // `cd` with no args goes home; `cd -` is not yet supported.
    let target = match argv.get(1) {
        Some(dir) => dir.clone(),
        None => match std::env::var("HOME") {
            Ok(h) => h,
            Err(_) => {
                eprintln!("cd: HOME not set");
                return 1;
            }
        },
    };

    if let Err(e) = std::env::set_current_dir(Path::new(&target)) {
        eprintln!("cd: {target}: {e}");
        return 1;
    }
    0
}

fn pwd() -> i32 {
    match std::env::current_dir() {
        Ok(p) => {
            println!("{}", p.display());
            0
        }
        Err(e) => {
            eprintln!("pwd: {e}");
            1
        }
    }
}

/// `export NAME` marks an existing variable exported; `export NAME=value` sets
/// and exports it. The `NAME=value` arg arrives already expanded.
fn export(argv: &[String]) -> i32 {
    for arg in &argv[1..] {
        match arg.split_once('=') {
            Some((name, value)) => crate::vars::set_exported(name, value),
            None => crate::vars::export(arg),
        }
    }
    0
}

/// `test EXPR` / `[ EXPR ]` — evaluate a conditional, returning 0 (true),
/// 1 (false), or 2 (usage error). Supports the common unary file/string tests,
/// string `=`/`!=`, integer `-eq`/`-ne`/`-lt`/`-le`/`-gt`/`-ge`, and a leading
/// `!`. (No `-a`/`-o`/parentheses yet.)
fn test_dispatch(argv: &[String], bracket: bool) -> i32 {
    let args: &[String] = if bracket {
        if argv.last().map(String::as_str) != Some("]") {
            eprintln!("[: missing `]'");
            return 2;
        }
        &argv[1..argv.len() - 1]
    } else {
        &argv[1..]
    };
    match test_eval(args) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(msg) => {
            eprintln!("test: {msg}");
            2
        }
    }
}

fn test_eval(args: &[String]) -> Result<bool, String> {
    match args {
        [] => Ok(false),
        // `! EXPR`
        [first, rest @ ..] if first == "!" => test_eval(rest).map(|b| !b),
        // A lone string: true when non-empty.
        [s] => Ok(!s.is_empty()),
        [op, operand] => test_unary(op, operand),
        [a, op, b] => test_binary(a, op, b),
        _ => Err("too many arguments".into()),
    }
}

fn test_unary(op: &str, s: &str) -> Result<bool, String> {
    use std::path::Path;
    Ok(match op {
        "-z" => s.is_empty(),
        "-n" => !s.is_empty(),
        "-e" => Path::new(s).exists(),
        "-f" => Path::new(s).is_file(),
        "-d" => Path::new(s).is_dir(),
        "-s" => Path::new(s).metadata().map(|m| m.len() > 0).unwrap_or(false),
        // Permission bits aren't portable; approximate with existence.
        "-r" | "-w" | "-x" => Path::new(s).exists(),
        _ => return Err(format!("unknown unary operator `{op}`")),
    })
}

fn test_binary(a: &str, op: &str, b: &str) -> Result<bool, String> {
    let int = |s: &str| s.parse::<i64>().map_err(|_| format!("integer expected: `{s}`"));
    Ok(match op {
        "=" | "==" => a == b,
        "!=" => a != b,
        "-eq" => int(a)? == int(b)?,
        "-ne" => int(a)? != int(b)?,
        "-lt" => int(a)? < int(b)?,
        "-le" => int(a)? <= int(b)?,
        "-gt" => int(a)? > int(b)?,
        "-ge" => int(a)? >= int(b)?,
        _ => return Err(format!("unknown operator `{op}`")),
    })
}

/// `break [n]` / `continue [n]` — request loop control; the executor acts on it.
fn loop_ctl(argv: &[String], is_break: bool) -> i32 {
    let n: u32 = argv.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    if n == 0 {
        eprintln!("{}: loop count must be positive", argv[0]);
        return 1;
    }
    let ctl = if is_break {
        crate::vars::LoopCtl::Break(n)
    } else {
        crate::vars::LoopCtl::Continue(n)
    };
    crate::vars::set_loop_ctl(Some(ctl));
    0
}

/// `return [n]` — unwind the current function with status `n` (default `$?`).
/// The executor's `call_function` consumes the request.
fn return_cmd(argv: &[String]) -> i32 {
    let code = argv
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(crate::vars::last_status);
    crate::vars::set_returning(Some(code));
    code
}

/// `unset NAME...` — remove shell variables.
fn unset(argv: &[String]) -> i32 {
    for name in &argv[1..] {
        crate::vars::unset(name);
    }
    0
}

/// `exit [n]` — request shell termination with status `n` (default `0`).
///
/// Vendored change (RFC 0002): instead of `std::process::exit` (which would kill
/// an embedding host and make the shell core untestable), this records the
/// request via [`crate::vars::set_exit_requested`]. The executor's `flow_pending`
/// early-out stops further commands, and `lib::eval` / `lib::run_repl` resolve
/// the latch into the process exit code.
fn exit(argv: &[String]) -> i32 {
    let code = argv
        .get(1)
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);
    crate::vars::set_exit_requested(Some(code));
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(args: &[&str]) -> Result<bool, String> {
        let v: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        test_eval(&v)
    }

    #[test]
    fn test_strings_and_negation() {
        assert_eq!(ev(&[]), Ok(false));
        assert_eq!(ev(&["abc"]), Ok(true));
        assert_eq!(ev(&[""]), Ok(false));
        assert_eq!(ev(&["-z", ""]), Ok(true));
        assert_eq!(ev(&["-n", "x"]), Ok(true));
        assert_eq!(ev(&["a", "=", "a"]), Ok(true));
        assert_eq!(ev(&["a", "!=", "b"]), Ok(true));
        assert_eq!(ev(&["!", "-z", "x"]), Ok(true));
    }

    #[test]
    fn test_integers() {
        assert_eq!(ev(&["3", "-lt", "5"]), Ok(true));
        assert_eq!(ev(&["5", "-eq", "5"]), Ok(true));
        assert_eq!(ev(&["5", "-ge", "9"]), Ok(false));
        assert!(ev(&["x", "-eq", "5"]).is_err());
    }

    #[test]
    fn test_files() {
        assert_eq!(ev(&["-f", "Cargo.toml"]), Ok(true));
        assert_eq!(ev(&["-d", "src"]), Ok(true));
        assert_eq!(ev(&["-e", "no-such-file-xyz"]), Ok(false));
    }
}
