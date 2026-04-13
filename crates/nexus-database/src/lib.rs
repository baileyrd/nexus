//! Nexus database engine: rich property types, query engine, formula language,
//! relations/rollups, and import/export.
//!
//! This crate wraps the existing `nexus-storage` bases infrastructure with a
//! full-featured database engine supporting 20+ property types, type-aware
//! validation, a Notion-compatible formula language, cross-database relations,
//! and CSV import/export.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod error;
pub mod schema;
pub mod types;
pub mod validate;

pub mod query;

pub mod formula;

pub mod core_plugin;
pub mod import_export;
pub mod relation;

pub use error::{DatabaseError, Result};
pub use schema::{SchemaMigration, SchemaOp, apply_migration, current_version, extract_configs, migration_history};
pub use types::{
    DateFormat, NumberFormat, PropertyConfig, PropertyValue, RollupAggregation, SelectOption,
};
pub use validate::{
    BuiltinValidator, PropertyValidator, Severity, ValidationIssue, validate_record_full,
};
pub use formula::{FormulaValue, evaluate as evaluate_formula};
pub use query::{
    Filter, FilterOp, Query, QueryResult, Sort, SortDirection, execute as execute_query,
    parse_filter, parse_sort,
};
pub use relation::{compute_rollup, resolve_relation};
pub use import_export::{ColumnMapping, ImportResult, export_csv, import_csv};
pub use core_plugin::DatabaseCorePlugin;
