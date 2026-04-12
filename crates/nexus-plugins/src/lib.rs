//! Nexus plugin system: manifest parsing, WASM sandbox, host functions,
//! plugin loader, settings validation, and hot-reload.
//!
//! See `docs/superpowers/specs/2026-04-12-nexus-prd-04-plugins-design.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod error;
pub mod manifest;
mod sandbox;

pub use error::PluginError;
pub use manifest::{
    CliSubcommandReg, EventSubscriberReg, IpcCommandReg, LifecycleConfig, ManifestCapabilities,
    PluginManifest, Registrations, SettingsConfig, WasmConfig,
};
pub use manifest::{load_manifest, parse_manifest, validate};
pub use sandbox::{PluginData, WasmSandbox};
