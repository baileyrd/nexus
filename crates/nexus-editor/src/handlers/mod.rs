//! Per-domain IPC handler modules for the editor core plugin.
//!
//! SD-03 editor split (2026-05-18 SOLID/DRY audit): migrates the
//! per-handler `handle_*` free functions out of `core_plugin.rs` into
//! thematic modules. Each module exposes `pub(crate)` entry points
//! named after the handler — e.g. `handlers::tree::get_tree(...)` for
//! `HANDLER_GET_TREE`. Cross-module helpers live in [`shared`].
//!
//! Mirrors the layout already used by `nexus-storage` / `nexus-git`
//! (see `crates/nexus-storage/src/handlers/mod.rs`).

pub(crate) mod save;
pub(crate) mod session;
pub(crate) mod shared;
pub(crate) mod transaction;
pub(crate) mod tree;
