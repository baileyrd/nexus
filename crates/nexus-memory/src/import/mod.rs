//! Importers that ingest external memory stores into the native engine.
//!
//! Each importer maps a foreign source into [`crate::model::Memory`] rows and
//! writes them through [`crate::db::MemoryDb`]. Embeddings are never copied —
//! vectors are recomputed by the `nexus-ai` path on demand (design D-1).

pub mod remind_me_db;

pub use remind_me_db::{import_remind_me_db, ImportReport};
