//! Stable plugin contract for Nexus.
//!
//! This crate contains only the types and traits that cross the kernel/plugin
//! boundary and must remain stable across kernel refactors. Plugin authors
//! should depend **only** on this crate, not on `nexus-kernel` or
//! `nexus-plugins` directly.
//!
//! ## What lives here
//!
//! - [`Capability`] / [`CapabilitySet`] — permission system
//! - [`TrustLevel`] / [`PluginInfo`] / [`PluginStatus`] — plugin identity
//! - [`NexusEvent`] / [`EventFilter`] / [`EventMetadata`] / [`PublishedEvent`] — events
//! - [`IpcDispatcher`] / [`IpcFuture`] — IPC abstractions
//! - [`IpcError`] / [`BusError`] / [`CapabilityError`] — stable error surface
//! - [`IpcErrorEnvelope`] / [`IpcErrorKind`] — wire-stable IPC error envelope
//! - [`LogLevel`] — log severity
//! - [`PLUGIN_API_VERSION`] — current ABI version constant
//!
//! ## What does NOT live here
//!
//! - `EventBus` / `EventSubscription` — broadcast transport (kernel-internal)
//! - `KernelPluginContext` — concrete kernel impl (kernel-internal)
//! - `PluginContext` trait — references `EventSubscription` (kernel-internal)
//! - `CorePlugin` / `PluginLoader` — plugin runtime (plugins-internal)
//! - `KvStore` / `InMemoryKvStore` — storage (kernel-internal)
//! - Config types, CLI types, storage types — crate-specific

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod capability;
pub mod error;
pub mod event;
pub mod ipc;
pub mod log;
pub mod plugin;

pub use capability::{Capability, CapabilityParseError, CapabilitySet};
pub use error::{BusError, CapabilityError, IpcError, IpcErrorEnvelope, IpcErrorKind};
pub use event::{EventFilter, EventMetadata, NexusEvent, PublishedEvent, StopReason};
pub use ipc::{IpcDispatcher, IpcFuture};
pub use log::LogLevel;
pub use plugin::{PluginInfo, PluginStatus, TrustLevel};

/// The host's current plugin API major version.
///
/// Manifests must declare `api_version = "1"` (or `"1.<minor>"`). The loader
/// rejects plugins whose major version differs from this constant.
///
/// Increment this only on breaking ABI changes. Minor extensions that remain
/// backwards-compatible increment the documented minor version in the spec
/// without changing this constant.
pub const PLUGIN_API_VERSION: u32 = 1;
