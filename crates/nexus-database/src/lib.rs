//! Nexus database support library: rich property types, type-aware
//! validation, a Notion-compatible formula language, and CSV import/export.
//!
//! This crate is a **pure-logic library** — it does not touch `rusqlite`
//! and is not a plugin. The SQL-backed query engine, schema migrations,
//! and relation/rollup resolution that previously lived here moved into
//! `nexus-storage` (`nexus_storage::bases::{schema, query, relation}`) so
//! that `nexus-storage` is the sole owner of the forge's SQLite database.
//!
//! Callers who need pure in-memory operations (formula evaluation, record
//! validation, CSV round-trip) can depend on this crate directly. Callers
//! who need to run SQL queries against bases should go through
//! `ipc_call("com.nexus.storage", "base_query", …)` instead.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod error;
pub mod formula;
pub mod import_export;
pub mod types;
pub mod validate;

pub use error::{DatabaseError, Result};
pub use formula::{FormulaValue, evaluate as evaluate_formula};
pub use import_export::{ColumnMapping, ImportResult, export_csv, import_csv};
pub use types::{
    DateFormat, NumberFormat, PropertyConfig, PropertyValue, RollupAggregation, SelectOption,
};
pub use validate::{
    BuiltinValidator, PropertyValidator, Severity, ValidationIssue, validate_record_full,
};
