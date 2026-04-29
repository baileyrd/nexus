//! Local embedding provider backed by `fastembed-rs` (BL-019, ADR 0018).
//!
//! Compiled only when the `local-embeddings` Cargo feature is enabled.
//! See [`LocalEmbedding`] for the [`crate::EmbeddingProvider`] implementor
//! and the in-process [`DashMap`](dashmap::DashMap) cache layer that
//! avoids recomputing embeddings for repeated input chunks across
//! re-indexing runs.

#![cfg(feature = "local-embeddings")]

// Skeleton only — the full implementation lands in commit 2.
// Keeping this file deliberately empty in the first commit lets us land
// the dependency wiring + feature flag in isolation and verify the
// default build still works without fastembed in the dep tree.
