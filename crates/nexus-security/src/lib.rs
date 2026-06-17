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
/// Permissioned download broker (approved egress for the network-off sandbox).
pub mod downloads;
mod error;
/// IPC wire types for `com.nexus.security`.
pub mod ipc;
/// OS process sandbox enforcement (Phase 4 F1) for `nexus_types::SandboxPolicy`.
pub mod os_sandbox;
mod path;
mod risk;
/// OS-sandbox configuration loaded from `<forge>/.forge/sandbox.toml`.
pub mod sandbox_config;
/// TLS pinning verifier for outbound HTTPS (BL-102).
pub mod tls;
/// Per-host TLS pin table (BL-102).
pub mod tls_pins;

pub use core_plugin::SecurityCorePlugin;
pub use credential::CredentialVault;
pub use downloads::{DownloadError, DownloadPolicy, DownloadRequest};
pub use error::SecurityError;
// Spawn-site helpers live in the leaf `nexus-types` (so a spawn site can wrap
// a command without linking this engine); re-exported here for convenience.
pub use nexus_types::{default_helper_path, sandbox_argv};
pub use os_sandbox::{
    apply_to_current_thread, block_inet_sockets, confine_current_thread, sandbox_command,
    NetworkStatus, SandboxError, SandboxStatus,
};
pub use sandbox_config::{SandboxConfig, SANDBOX_CONFIG_RELPATH};
pub use path::ForgePathValidator;
pub use risk::{risk_level, RiskLevel};
