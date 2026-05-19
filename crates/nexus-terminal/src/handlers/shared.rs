//! Cross-cutting helper re-exports for terminal handler modules.
//!
//! Mirrors `crates/nexus-storage/src/handlers/shared.rs`: the dispatch
//! helpers + a couple of small terminal-specific error adapters are
//! defined `pub(crate)` once in `core_plugin.rs` and re-exported here
//! so handler modules don't have to reach across into the parent
//! module.

pub(crate) use crate::core_plugin::{crate_err, exec_err, parse_args, poisoned, to_value};
