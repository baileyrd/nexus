//! Audit-2026-05-01 P2-2 — shared `MinimalForge` test fixture.
//!
//! Reduces setup boilerplate across the per-subsystem `*_ipc.rs`
//! integration tests under `crates/nexus-bootstrap/tests/`. Each test
//! file pulls this module in via the standard Rust integration-test
//! pattern:
//!
//! ```ignore
//! #[path = "common/mod.rs"]
//! mod common;
//! use common::MinimalForge;
//! ```
//!
//! Cargo treats every `tests/*.rs` file as a separate crate, so a
//! `mod common;` written here would be silently unused by some test
//! binaries. The `#[path = "..."] mod` attribute hoists it into each
//! test crate where it's needed; unused-module dead-code warnings are
//! suppressed below.
//!
//! ## What this gives you
//!
//! - `MinimalForge::new()` — empty tempdir-backed forge + a built CLI
//!   runtime, paired in one struct so the tempdir survives as long as
//!   the runtime needs it.
//! - `MinimalForge::with_files(&[("path", b"content")])` — seed
//!   markdown (or any bytes) before the runtime boots.
//! - `MinimalForge::ipc_call(plugin_id, command, args)` — single-line
//!   wrapper over `runtime.context.ipc_call` with the standard
//!   10-second timeout already applied.

#![allow(dead_code)] // Each test crate uses a different subset of these.

use std::path::{Path, PathBuf};
use std::time::Duration;

use nexus_bootstrap::{build_cli_runtime, Runtime};
use nexus_kernel::{Ipc as _, IpcError};

/// Default timeout used by [`MinimalForge::ipc_call`]. Generous enough
/// for IPC paths that touch the SQLite index without flaking on slow
/// CI runners; still finite so a wedged handler eventually surfaces
/// as `IpcError::Timeout` rather than hanging the test binary.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// Bundled tempdir + boot-up Nexus runtime. Drop order matters — the
/// tempdir field is declared after the runtime so the runtime
/// (which holds open file handles into the forge) shuts down first
/// when the struct is dropped.
pub struct MinimalForge {
    /// Fully-booted CLI runtime pointing at the tempdir below. Use
    /// `runtime.context.ipc_call(...)` directly for anything the
    /// helper [`ipc_call`](Self::ipc_call) doesn't cover (e.g.
    /// custom timeouts, event subscriptions, kv access).
    pub runtime: Runtime,
    /// Backing tempdir. Held to keep the forge alive — Rust drops
    /// fields top-to-bottom in declaration order, so `runtime` (and
    /// any background tasks holding file handles) is dropped before
    /// `tempdir`'s `Drop` impl wipes the directory.
    pub tempdir: tempfile::TempDir,
}

impl MinimalForge {
    /// Build a forge with no user files seeded.
    ///
    /// # Panics
    /// Panics if tempdir creation, forge init, or runtime build fails.
    /// Test code wants loud failures here — a silent setup error makes
    /// every downstream assertion misleading.
    pub fn new() -> Self {
        Self::with_files(&[])
    }

    /// Build a forge with the supplied files written before the runtime
    /// boots. Paths are forge-relative; intermediate directories are
    /// created as needed.
    ///
    /// # Panics
    /// Panics if any directory or file write fails. See [`Self::new`]
    /// for the rationale.
    pub fn with_files(files: &[(&str, &[u8])]) -> Self {
        let tempdir = tempfile::tempdir().expect("MinimalForge: tempdir creation failed");
        nexus_storage::StorageEngine::init(tempdir.path())
            .expect("MinimalForge: forge init failed");

        for (relpath, bytes) in files {
            let abs = tempdir.path().join(relpath);
            if let Some(parent) = abs.parent() {
                std::fs::create_dir_all(parent)
                    .unwrap_or_else(|e| panic!("MinimalForge: mkdir {}: {e}", parent.display()));
            }
            std::fs::write(&abs, bytes)
                .unwrap_or_else(|e| panic!("MinimalForge: write {}: {e}", abs.display()));
        }

        let runtime = build_cli_runtime(tempdir.path().to_path_buf())
            .expect("MinimalForge: build_cli_runtime failed");

        Self { runtime, tempdir }
    }

    /// Forge root path. Convenience for filesystem-side assertions
    /// (e.g. `assert!(forge.root().join("notes/x.md").exists())`).
    #[must_use]
    pub fn root(&self) -> &Path {
        self.tempdir.path()
    }

    /// Owned `PathBuf` form of [`Self::root`]. Useful when a helper
    /// signature needs an owned path.
    #[must_use]
    pub fn root_owned(&self) -> PathBuf {
        self.tempdir.path().to_path_buf()
    }

    /// Issue an IPC call against the booted runtime with the standard
    /// [`CALL_TIMEOUT`] applied. Returns the raw `serde_json::Value`
    /// reply — callers that want a typed result can pipe through
    /// [`serde_json::from_value`].
    ///
    /// # Errors
    /// Propagates the kernel-side [`IpcError`] (timeout, plugin not
    /// found, command not found, plugin crashed, etc.).
    pub async fn ipc_call(
        &self,
        plugin_id: &str,
        command: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, IpcError> {
        self.runtime
            .context
            .ipc_call(plugin_id, command, args, CALL_TIMEOUT)
            .await
    }
}
