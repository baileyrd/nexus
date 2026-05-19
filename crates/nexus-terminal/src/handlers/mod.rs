//! Per-domain IPC handler modules for the terminal core plugin.
//!
//! SD-03 terminal split (2026-05-18 SOLID/DRY audit): migrates the
//! `dispatch_*` instance methods out of `core_plugin.rs` into thematic
//! `impl TerminalCorePlugin { ... }` blocks per domain. Mirrors the
//! `handlers/<domain>.rs` layout used by `nexus-storage` /
//! `nexus-git` / `nexus-editor`.

pub(crate) mod adhoc;
pub(crate) mod info;
pub(crate) mod saved;
pub(crate) mod session;
pub(crate) mod shared;
