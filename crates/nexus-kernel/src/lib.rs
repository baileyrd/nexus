//! Nexus kernel: event bus, plugin lifecycle, capability system.
//!
//! See `docs/superpowers/specs/2026-04-11-nexus-prd-01-kernel-interface-spec.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod capability;
mod config;
mod context;
mod error;
mod event;
mod event_bus;
mod log;
mod plugin;
mod plugin_registry;

pub use capability::{Capability, CapabilityParseError, CapabilitySet};
pub use config::KernelConfig;
pub use context::PluginContext;
pub use error::{
    BusError, CapabilityError, ConfigError, Error, IpcError, KvError, PluginError, RecvError,
    Result,
};
pub use event::{EventFilter, EventMetadata, NexusEvent, PublishedEvent, StopReason};
pub use event_bus::{EventBus, EventSubscription};
pub use log::LogLevel;
pub use plugin::{PluginInfo, PluginLifecycle, PluginStatus, TrustLevel};
pub use plugin_registry::PluginRegistry;
