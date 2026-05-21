//! Shared constants used across Nexus crates.
//!
//! Centralizes magic numbers — primarily IPC timeouts — so frontends don't
//! drift apart on what "long" or "short" means for a kernel round-trip.

use std::time::Duration;

/// Short IPC timeout — interactive CLI/UI calls that should complete quickly
/// (tool dispatch, single notification send, lightweight queries, log fetch,
/// db ops, proc ops, skill / workflow read paths).
pub const IPC_TIMEOUT_SHORT: Duration = Duration::from_secs(30);

/// Normal IPC timeout — service plugins that may make a single outbound
/// hop (MCP host bridging into other plugins).
pub const IPC_TIMEOUT_NORMAL: Duration = Duration::from_secs(60);

/// Long IPC timeout — operations that may block on model/network IO
/// or larger filesystem work (AI calls, graph rebuild).
pub const IPC_TIMEOUT_LONG: Duration = Duration::from_secs(120);

/// Extended IPC timeout — long-running orchestration where the caller is
/// willing to wait minutes (agent runs, full-forge sync, workflow runs).
pub const IPC_TIMEOUT_EXTENDED: Duration = Duration::from_secs(600);

/// Audit log retention horizon — entries older than this are eligible for
/// pruning by the security/audit subsystem.
pub const AUDIT_LOG_RETENTION_DAYS: u32 = 90;

/// Maximum number of results the command palette will surface for a query
/// before clamping further matches.
pub const COMMAND_PALETTE_MAX_RESULTS: usize = 50;

/// Maximum number of blocking threads the kernel's host tokio runtime
/// should keep around for `spawn_blocking` work.
///
/// The IPC dispatcher falls back to `spawn_blocking` for sync handlers
/// (`crates/nexus-kernel/src/context_impl.rs`), so this caps the number
/// of concurrent sync IPC calls plus all other blocking work
/// (git worker, terminal sessions, AI embeddings, etc.). Tokio's
/// default of 512 lets a runaway plugin pile up gigabytes of stack
/// before saturation; 64 is comfortable headroom for a desktop or CLI
/// workload while keeping the blast radius bounded.
///
/// Frontends pass this to `tokio::runtime::Builder::max_blocking_threads`
/// at construction time. The kernel itself does not own a runtime —
/// it is hosted in whatever runtime the frontend provided.
pub const KERNEL_BLOCKING_POOL_SIZE: usize = 64;

/// Warn threshold for the kernel's in-flight sync IPC dispatch counter.
///
/// When sustained sync-dispatch depth crosses this number the kernel
/// emits a one-shot warn (deduplicated until depth drops below half
/// the threshold) so operators see saturation building before the
/// pool is exhausted. Set to 75 % of [`KERNEL_BLOCKING_POOL_SIZE`].
pub const KERNEL_BLOCKING_POOL_WARN_DEPTH: usize = 48;
