//! Shell function definitions.
//!
//! A `name() { ... }` definition stores its body (a `CommandList`) here; calling
//! the function runs that body with the call's arguments as `$1`…. Like the rest
//! of the shell's mutable state, the registry is a thread-local.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::parser::CommandList;

thread_local! {
    static FUNCS: RefCell<HashMap<String, CommandList>> = RefCell::new(HashMap::new());
}

/// Define (or redefine) a function.
pub fn define(name: &str, body: CommandList) {
    FUNCS.with(|f| {
        f.borrow_mut().insert(name.to_string(), body);
    });
}

/// Whether a function by this name is defined.
pub fn exists(name: &str) -> bool {
    FUNCS.with(|f| f.borrow().contains_key(name))
}

/// A clone of a function's body, for execution.
pub fn get(name: &str) -> Option<CommandList> {
    FUNCS.with(|f| f.borrow().get(name).cloned())
}

/// Remove every function definition (part of `vars::reset_state`).
pub fn clear() {
    FUNCS.with(|f| f.borrow_mut().clear());
}
