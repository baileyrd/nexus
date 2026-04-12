//! Nexus security: capability risk metadata, credential vault, audit logging,
//! and forge path validation.
//!
//! See `docs/superpowers/specs/2026-04-12-nexus-prd-02-security-design.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod audit;
mod credential;
mod error;
mod path;
mod risk;

pub use credential::CredentialVault;
pub use error::SecurityError;
pub use path::ForgePathValidator;
pub use risk::{risk_level, RiskLevel};
