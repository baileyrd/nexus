//! Shell state that outlives a single command: the last exit status (`$?`) and
//! shell variables (`FOO=bar`, `export`).
//!
//! The REPL is single-threaded, so a thread-local `RefCell` is all the
//! synchronisation we need — the same approach `job` uses for its job table.
//!
//! Variables live only in this map, not the process environment. Lookups for
//! `$VAR` consult the map first and fall back to the real environment, and only
//! variables marked *exported* are pushed into child processes (see
//! `exec::build_stage`). Non-exported variables stay private to the shell.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

struct Var {
    value: String,
    exported: bool,
}

/// A pending `break`/`continue` request, carrying how many enclosing loops it
/// applies to (`break 2`). The executor consumes it level by level.
#[derive(Clone, Copy)]
pub enum LoopCtl {
    Break(u32),
    Continue(u32),
}

thread_local! {
    static LAST_STATUS: RefCell<i32> = const { RefCell::new(0) };
    static VARS: RefCell<HashMap<String, Var>> = RefCell::new(HashMap::new());
    static LOOP_CTL: RefCell<Option<LoopCtl>> = const { RefCell::new(None) };
    static RETURNING: RefCell<Option<i32>> = const { RefCell::new(None) };
    // `$0` (shell/script name) and `$1`, `$2`, … (positional parameters).
    static SHELL_NAME: RefCell<String> = RefCell::new("rush".to_string());
    static ARGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    // A pending `exit [n]` request. The `exit` builtin sets this instead of
    // calling `std::process::exit`, so the shell core stays embeddable; the
    // executor stops further commands and `lib::eval` returns the code.
    static EXIT_REQUESTED: RefCell<Option<i32>> = const { RefCell::new(None) };
    // Whether the shell is embedded inside a Nexus-owned PTY. When set, job
    // control must not claim the controlling terminal (the PTY session leader
    // already owns it). Host configuration, not per-run shell state.
    static EMBEDDED: Cell<bool> = const { Cell::new(false) };
}

/// Record (or clear) a pending `exit [n]` request.
pub fn set_exit_requested(code: Option<i32>) {
    EXIT_REQUESTED.with(|e| *e.borrow_mut() = code);
}

/// The pending `exit` code, if the `exit` builtin ran.
pub fn exit_requested() -> Option<i32> {
    EXIT_REQUESTED.with(|e| *e.borrow())
}

/// Mark whether the shell is running embedded inside a Nexus-owned PTY.
pub fn set_embedded(embedded: bool) {
    EMBEDDED.with(|e| e.set(embedded));
}

/// Whether the shell is running embedded (job-control terminal hand-off off).
pub fn embedded() -> bool {
    EMBEDDED.with(Cell::get)
}

/// Reset all thread-local shell state to its initial values. Used by
/// `lib::eval_fresh` so a one-shot evaluation cannot observe a prior run's
/// variables, `$?`, positional parameters, or function definitions. The
/// [`embedded`] flag is host configuration and is intentionally preserved.
pub fn reset_state() {
    LAST_STATUS.with(|s| *s.borrow_mut() = 0);
    VARS.with(|v| v.borrow_mut().clear());
    LOOP_CTL.with(|c| *c.borrow_mut() = None);
    RETURNING.with(|r| *r.borrow_mut() = None);
    SHELL_NAME.with(|n| *n.borrow_mut() = "rush".to_string());
    ARGS.with(|a| a.borrow_mut().clear());
    EXIT_REQUESTED.with(|e| *e.borrow_mut() = None);
    crate::func::clear();
    // The job table is per-run state too: a fresh eval must not see a prior
    // run's background jobs. (Terminal/job-control setup is preserved — see
    // `job::reset`.)
    #[cfg(unix)]
    crate::job::reset();
}

/// Set `$0` and the positional parameters (`$1`…).
pub fn set_args(name: String, args: Vec<String>) {
    SHELL_NAME.with(|n| *n.borrow_mut() = name);
    ARGS.with(|a| *a.borrow_mut() = args);
}

/// `$n`: `$0` is the shell/script name, `$1`… the positional parameters.
pub fn arg(n: usize) -> Option<String> {
    if n == 0 {
        Some(SHELL_NAME.with(|s| s.borrow().clone()))
    } else {
        ARGS.with(|a| a.borrow().get(n - 1).cloned())
    }
}

/// `$#` — the number of positional parameters.
pub fn arg_count() -> usize {
    ARGS.with(|a| a.borrow().len())
}

/// All positional parameters (`$@` / `$*`).
pub fn args() -> Vec<String> {
    ARGS.with(|a| a.borrow().clone())
}

/// Record a pending loop-control request (from the `break`/`continue` builtins).
pub fn set_loop_ctl(ctl: Option<LoopCtl>) {
    LOOP_CTL.with(|c| *c.borrow_mut() = ctl);
}

/// The pending loop-control request, if any.
pub fn loop_ctl() -> Option<LoopCtl> {
    LOOP_CTL.with(|c| *c.borrow())
}

/// Record a pending `return` (from the `return` builtin) with its exit code.
pub fn set_returning(code: Option<i32>) {
    RETURNING.with(|r| *r.borrow_mut() = code);
}

/// The pending `return` code, if a function should unwind.
pub fn returning() -> Option<i32> {
    RETURNING.with(|r| *r.borrow())
}

/// Whether any non-local control flow (`break`/`continue`/`return`/`exit`) is
/// pending, so a list should stop running further commands.
pub fn flow_pending() -> bool {
    loop_ctl().is_some() || returning().is_some() || exit_requested().is_some()
}

/// The exit status of the most recently completed pipeline — exposed as `$?`.
pub fn last_status() -> i32 {
    LAST_STATUS.with(|s| *s.borrow())
}

pub fn set_last_status(code: i32) {
    LAST_STATUS.with(|s| *s.borrow_mut() = code);
}

/// Look up a shell variable's value (not the environment — see `expand`).
pub fn get(name: &str) -> Option<String> {
    VARS.with(|v| v.borrow().get(name).map(|x| x.value.clone()))
}

/// Remove a shell variable (`unset NAME`).
pub fn unset(name: &str) {
    VARS.with(|v| {
        v.borrow_mut().remove(name);
    });
}

/// Set a variable, preserving its exported flag if it already existed.
pub fn set(name: &str, value: &str) {
    VARS.with(|v| {
        let mut m = v.borrow_mut();
        let exported = m.get(name).is_some_and(|x| x.exported);
        m.insert(name.to_string(), Var { value: value.to_string(), exported });
    });
}

/// Set a variable and mark it exported (`export NAME=value`).
pub fn set_exported(name: &str, value: &str) {
    VARS.with(|v| {
        v.borrow_mut().insert(
            name.to_string(),
            Var { value: value.to_string(), exported: true },
        );
    });
}

/// Mark an existing (or newly-created, empty) variable exported (`export NAME`).
pub fn export(name: &str) {
    VARS.with(|v| {
        v.borrow_mut()
            .entry(name.to_string())
            .or_insert_with(|| Var { value: String::new(), exported: false })
            .exported = true;
    });
}

/// A snapshot of all variables, for isolating a subshell.
pub type Snapshot = Vec<(String, String, bool)>;

pub fn snapshot() -> Snapshot {
    VARS.with(|v| {
        v.borrow()
            .iter()
            .map(|(k, x)| (k.clone(), x.value.clone(), x.exported))
            .collect()
    })
}

pub fn restore(snap: Snapshot) {
    VARS.with(|v| {
        let mut m = v.borrow_mut();
        m.clear();
        for (name, value, exported) in snap {
            m.insert(name, Var { value, exported });
        }
    });
}

/// Every exported variable as `(name, value)`, for seeding child environments.
pub fn exported() -> Vec<(String, String)> {
    VARS.with(|v| {
        v.borrow()
            .iter()
            .filter(|(_, x)| x.exported)
            .map(|(k, x)| (k.clone(), x.value.clone()))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_unset_and_export() {
        set("RUSH_V", "1");
        assert_eq!(get("RUSH_V").as_deref(), Some("1"));
        assert!(!exported().iter().any(|(k, _)| k == "RUSH_V"));

        export("RUSH_V");
        assert!(exported().iter().any(|(k, v)| k == "RUSH_V" && v == "1"));

        // Re-setting keeps the exported flag.
        set("RUSH_V", "2");
        assert!(exported().iter().any(|(k, v)| k == "RUSH_V" && v == "2"));

        unset("RUSH_V");
        assert_eq!(get("RUSH_V"), None);
    }
}
