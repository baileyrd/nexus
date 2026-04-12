//! Nexus kernel: event bus, plugin lifecycle, capability system.
//!
//! See `docs/superpowers/specs/2026-04-11-nexus-prd-01-kernel-interface-spec.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod capability;
mod config;
mod event;
mod log;

pub use capability::{Capability, CapabilityParseError, CapabilitySet};
pub use config::KernelConfig;
pub use event::{EventFilter, EventMetadata, NexusEvent, StopReason};
pub use log::LogLevel;
