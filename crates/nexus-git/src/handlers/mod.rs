//! Per-domain IPC handler modules for the git core plugin.
//!
//! `core_plugin.rs` keeps the `CorePlugin` trait impl + the plugin
//! struct + lifecycle hooks, and dispatches into these modules by
//! handler id. Cross-cutting helpers (`path_arg`, `hunk_indices_arg`,
//! `map_err`, etc.) live in [`shared`].
//!
//! Decomposed from the former monolithic `core_plugin.rs` per the
//! 2026-05-18 SOLID/DRY audit SD-03. Groupings are thematic rather
//! than per-handler so the file count stays manageable (~7 files for
//! 38 handlers).

pub(crate) mod branches;
pub(crate) mod log;
pub(crate) mod merge;
pub(crate) mod shared;
pub(crate) mod stash;
pub(crate) mod staging;
pub(crate) mod status;
pub(crate) mod tags;
