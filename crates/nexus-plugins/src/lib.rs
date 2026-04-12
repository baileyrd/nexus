//! Nexus plugin system: manifest parsing, WASM sandbox, host functions,
//! plugin loader, settings validation, and hot-reload.
//!
//! See `docs/superpowers/specs/2026-04-12-nexus-prd-04-plugins-design.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod error;
mod host_fns;
pub mod manifest;
mod loader;
mod sandbox;
mod settings;

/// Trait for key-value storage backends. Implemented by the kernel.
/// Namespace is the plugin ID — plugins cannot access each other's data.
pub trait KvStore: Send + Sync {
    /// Get a value by key within a namespace.
    ///
    /// # Errors
    /// Returns [`PluginError`] if the storage backend encounters an I/O or
    /// internal error.
    fn get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, PluginError>;

    /// Set a value by key within a namespace.
    ///
    /// # Errors
    /// Returns [`PluginError`] if the storage backend encounters an I/O or
    /// internal error.
    fn set(&self, namespace: &str, key: &str, value: &[u8]) -> Result<(), PluginError>;

    /// Delete a key within a namespace.
    ///
    /// # Errors
    /// Returns [`PluginError`] if the storage backend encounters an I/O or
    /// internal error.
    fn delete(&self, namespace: &str, key: &str) -> Result<(), PluginError>;
}

pub use error::PluginError;
pub use loader::PluginLoader;
pub use manifest::{
    CliSubcommandReg, EventSubscriberReg, IpcCommandReg, LifecycleConfig, ManifestCapabilities,
    PluginManifest, Registrations, SettingsConfig, WasmConfig,
};
pub use manifest::{load_manifest, parse_manifest, validate};
pub use sandbox::{PluginData, WasmSandbox};
pub use settings::SettingsManager;
