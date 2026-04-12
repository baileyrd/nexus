//! Nexus plugin system: manifest parsing, WASM sandbox, host functions,
//! plugin loader, settings validation, and hot-reload.
//!
//! See `docs/superpowers/specs/2026-04-12-nexus-prd-04-plugins-design.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod error;

pub use error::PluginError;
