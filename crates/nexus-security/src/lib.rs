//! Nexus security: capability risk metadata, credential vault, audit logging,
//! and forge path validation.
//!
//! See `docs/superpowers/specs/2026-04-12-nexus-prd-02-security-design.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

/// Structured audit event helpers. Re-exported from `nexus-kernel` so call
/// sites in the kernel and plugin host can emit events without inducing a
/// dep cycle through `nexus-security`.
pub use nexus_kernel::audit;

/// Core plugin (`com.nexus.security`) and IPC handler constants.
pub mod core_plugin;
mod credential;
mod error;
/// IPC wire types for `com.nexus.security`.
pub mod ipc;
/// OS process sandbox enforcement (Phase 4 F1) for `nexus_types::SandboxPolicy`.
pub mod os_sandbox;
mod path;
mod risk;
/// TLS pinning verifier for outbound HTTPS (BL-102).
pub mod tls;
/// Per-host TLS pin table (BL-102).
pub mod tls_pins;

pub use core_plugin::SecurityCorePlugin;
pub use credential::CredentialVault;
pub use error::SecurityError;
pub use os_sandbox::{
    apply_to_current_thread, block_inet_sockets, confine_current_thread, default_helper_path,
    sandbox_command, NetworkStatus, SandboxError, SandboxStatus,
};
pub use path::ForgePathValidator;
pub use risk::{risk_level, RiskLevel};
