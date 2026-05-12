//! Nexus database support library: rich property types, type-aware
//! validation, a Notion-compatible formula language, and CSV import/export.
//!
//! This crate is **pure-logic** — it does not touch `rusqlite`. The
//! SQL-backed query engine, schema migrations, and relation/rollup
//! resolution that previously lived here moved into `nexus-storage`
//! (`nexus_storage::bases::{schema, query, relation}`) so that
//! `nexus-storage` is the sole owner of the forge's `SQLite` database.
//!
//! The crate also exposes a thin [`core_plugin::DatabaseCorePlugin`] that
//! surfaces its pure helpers (CSV import/export, formula evaluation) over
//! IPC as `com.nexus.database`. Invokers (CLI / TUI) reach these via
//! `ipc_call("com.nexus.database", …)` rather than linking the library
//! directly (invariant #3 in `docs/architecture/C4.md` §7).
//!
//! Callers who need to run SQL queries against bases should go through
//! `ipc_call("com.nexus.storage", "base_query", …)` instead.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod core_plugin;
pub mod error;
pub mod formula;
pub mod import_export;
pub mod types;
pub mod validate;
pub mod views;

pub use core_plugin::DatabaseCorePlugin;
pub use error::{DatabaseError, Result};
pub use formula::{FormulaValue, evaluate as evaluate_formula};
pub use import_export::{ColumnMapping, ImportResult, export_csv, import_csv};
pub use types::{
    DateFormat, NumberFormat, PropertyConfig, PropertyValue, RollupAggregation, SelectOption,
};
pub use validate::{
    BuiltinValidator, PropertyValidator, Severity, ValidationIssue, validate_record_full,
};
pub use views::{
    apply_view, validate_filter_operator, AppliedView, ViewGroup, ViewLayout, MISSING_GROUP_KEY,
};
