//! Nexus security: capability risk metadata, credential vault, audit logging,
//! and forge path validation.
//!
//! See `docs/superpowers/specs/2026-04-12-nexus-prd-02-security-design.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod audit;
mod error;
mod risk;

pub use error::SecurityError;
pub use risk::{risk_level, RiskLevel};
