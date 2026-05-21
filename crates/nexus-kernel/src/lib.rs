//! Nexus kernel: event bus, plugin lifecycle, capability system.
//!
//! See `docs/superpowers/specs/2026-04-11-nexus-prd-01-kernel-interface-spec.md`
//! for the public contract this crate implements.
//!
//! # Stable vs kernel-internal surface
//!
//! Types needed by **plugin authors** live in `nexus-plugin-api` and are
//! re-exported here for convenience. Kernel-internal types (`EventBus`,
//! `KernelPluginContext`, `KvStore`) stay in this crate.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod audit;
pub mod audit_store;
/// Cooperative IPC cancellation — task-local signal pipe + accessor.
pub mod cancel;
mod capability;
mod config;
mod context;
mod context_impl;
mod error;
mod event;
mod event_bus;
mod ipc;
mod kernel;
mod kv_store;
mod log;
/// BL-093: kernel-side metrics registry.
pub mod metrics;
mod plugin;

// Stable plugin-api types (defined in nexus-plugin-api, shim-re-exported via
// the local module files above so `crate::*` imports inside this crate work).
pub use capability::{Capability, CapabilityParseError, CapabilitySet};
pub use event::{EventFilter, EventMetadata, NexusEvent, PublishedEvent, StopReason};
pub use ipc::{IpcDispatcher, IpcFuture};
pub use log::LogLevel;
pub use plugin::{PluginInfo, PluginStatus, TrustLevel};

// Also re-export the version constant and the nexus-plugin-api error types
// so downstream crates that currently import from nexus-kernel keep compiling.
pub use nexus_plugin_api::{
    BusError, CapabilityError, IpcError, IpcErrorEnvelope, IpcErrorKind, PLUGIN_API_VERSION,
};

// Kernel-internal types
pub use config::{KernelConfig, WasmCapsCeiling};
pub use context::{Events, FileSystem, Identity, Ipc, KvAccess, Log, PluginContext};
pub use context_impl::KernelPluginContext;
pub use error::{ConfigError, Error, KvError, PluginError, RecvError, Result};
pub use event_bus::{type_id_in_namespace, EventBus, EventSubscription};
pub use kernel::Kernel;
pub use metrics::{CallStatus, HistogramSnapshot, KernelMetrics, MetricsSnapshot};
pub use kv_store::{InMemoryKvStore, KvStore};

// Cooperative cancellation accessor for handler opt-in. See [`cancel`].
pub use cancel::ipc_cancel_token;
