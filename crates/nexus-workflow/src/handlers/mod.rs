//! Per-handler modules for the `com.nexus.workflow` core plugin.
//!
//! `core_plugin.rs` keeps the `CorePlugin` trait impl, the
//! `WorkflowCorePlugin` struct, the `HANDLER_*` constants, the
//! plugin-lifetime trigger / scheduler wiring, and the IPC arg types.
//! Per-handler bodies live here; `dispatch` / `dispatch_async`
//! delegate to `handlers::<name>::handle*`.
//!
//! Cross-cutting helpers (error converters, `parse` / `to_value`
//! adapters, the activity-timeline publisher) live in
//! [`shared`].

pub(crate) mod digest;
pub(crate) mod get;
pub(crate) mod list;
pub(crate) mod next_fire;
pub(crate) mod reload;
pub(crate) mod run;
pub(crate) mod run_history;
pub(crate) mod shared;
pub(crate) mod templates;
pub(crate) mod validate;
