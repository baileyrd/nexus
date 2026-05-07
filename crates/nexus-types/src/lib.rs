//! Nexus shared types.
//!
//! This crate is the leaf of the Nexus dependency graph. It holds types
//! that must be shared between the kernel (`nexus-kernel`) and plugin code
//! that runs in WASM sandboxes, and between subsystems that would otherwise
//! form a dependency cycle.
//!
//! See `docs/superpowers/specs/2026-04-11-nexus-prd-01-kernel-interface-spec.md`
//! for the contract this crate supports.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]

pub mod activity;
pub mod bases;
pub mod obsidian_base;
pub mod path_validator;
pub mod paths;

pub use path_validator::{ForgePathValidator, PathValidationError};
