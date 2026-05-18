//! Per-domain IPC handler modules for the storage core plugin.
//!
//! Phase A of the SD-03 storage split (2026-05-18 SOLID/DRY audit):
//! migrates the dozen already-factored `dispatch_<verb>` fns out of
//! `core_plugin.rs` into thematic modules. The remaining inline match
//! arms stay in `core_plugin.rs` for now (Phase B follow-on).
//!
//! Each module exposes `pub(crate)` entry points named after the
//! handler — e.g. `handlers::entity::search(...)` for
//! `HANDLER_ENTITY_SEARCH`. Helpers shared across modules live in
//! [`shared`].

pub(crate) mod canvas;
pub(crate) mod config;
pub(crate) mod entity;
pub(crate) mod files;
pub(crate) mod graph;
pub(crate) mod index;
pub(crate) mod notes;
pub(crate) mod search;
pub(crate) mod shared;
pub(crate) mod tasks;
pub(crate) mod tree;
pub(crate) mod vector;
