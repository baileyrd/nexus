# Nexus PRD 02 — Security Model (M1-Slimmed) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `nexus-security` crate to interface-complete state — capability risk-level metadata, generic credential vault (keyring-rs), structured audit logging (tracing), and forge path validation, all compiling and tested.

**Architecture:** New `nexus-security` workspace member that depends on `nexus-kernel` (for `Capability` enum) and `nexus-types`. Four focused modules (`risk`, `credential`, `audit`, `path`) plus a `SecurityError` enum. No runtime state — this crate provides pure functions and thin wrappers, consumed by the kernel's `PluginContext` impl and the CLI binary.

**Tech Stack:** Rust (edition 2024), `keyring` 3.x, `tracing` 0.1, `thiserror` 2.0, `serde` 1.0, `tempfile` 3.x (dev-dep for path tests).

**Parent docs:**
- [`2026-04-12-nexus-prd-02-security-design.md`](../specs/2026-04-12-nexus-prd-02-security-design.md) — **the contract this plan implements**
- [`2026-04-11-nexus-m1-foundation-spec.md`](../specs/2026-04-11-nexus-m1-foundation-spec.md) — M1 spec §9

---

## Prerequisites

1. PRD 01 (kernel crate) is complete and tests pass.
2. Verify: `cargo nextest run --workspace` passes with no failures.
3. The `Capability` enum in `nexus-kernel` has `as_str()`, `from_str()`, and `Capability::ALL`.

---

## File Structure

```
crates/nexus-security/
├── Cargo.toml
└── src/
    ├── lib.rs              # crate-level docs, public re-exports
    ├── error.rs            # SecurityError enum
    ├── risk.rs             # RiskLevel + risk_level() mapping
    ├── credential.rs       # CredentialVault (keyring-rs wrapper)
    ├── audit.rs            # structured audit event helpers
    └── path.rs             # ForgePathValidator
```

Modifications to existing files:
- `Cargo.toml` (workspace root): add `nexus-security` to members, add `keyring` and `tempfile` to workspace deps
- `crates/nexus-kernel/src/capability.rs`: add `Display` impl for `Capability` (needed by `SecurityError::CapabilityDenied`)

---

## Task Overview

13 tasks across 7 phases:
1. Phase 1: Crate skeleton + workspace wiring (Tasks 1–2)
2. Phase 2: Display impl on Capability (Task 3)
3. Phase 3: SecurityError enum (Task 4)
4. Phase 4: RiskLevel + risk_level mapping (Tasks 5–6)
5. Phase 5: Structured audit logging (Tasks 7–8)
6. Phase 6: Forge path validation (Tasks 9–10)
7. Phase 7: Credential vault (Tasks 11–13)

---

## Phase 1: Crate Skeleton

### Task 1: Add nexus-security to workspace and create crate skeleton

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Create: `crates/nexus-security/Cargo.toml`
- Create: `crates/nexus-security/src/lib.rs`

- [ ] **Step 1: Add workspace member and deps to root `Cargo.toml`**

Edit `/mnt/c/Users/baile/dev/nexus/Cargo.toml`:

In the `[workspace]` members array, add `"crates/nexus-security"`:

```toml
[workspace]
resolver = "2"
members = [
    "crates/nexus-types",
    "crates/nexus-kernel",
    "crates/nexus-security",
]
```

In `[workspace.dependencies]`, add:

```toml
# Keyring (OS credential storage)
keyring = "3"

# Test utilities
tempfile = "3"
```

- [ ] **Step 2: Create `crates/nexus-security/Cargo.toml`**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-security/Cargo.toml`:

```toml
[package]
name = "nexus-security"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true
description = "Nexus security: capability risk metadata, credential vault, audit logging, path validation"

[dependencies]
nexus-kernel = { path = "../nexus-kernel" }
nexus-types = { path = "../nexus-types" }
tracing = { workspace = true }
thiserror = { workspace = true }
serde = { workspace = true }
keyring = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
tracing-subscriber = { workspace = true }
```

- [ ] **Step 3: Create `crates/nexus-security/src/lib.rs`**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-security/src/lib.rs`:

```rust
//! Nexus security: capability risk metadata, credential vault, audit logging,
//! and forge path validation.
//!
//! See `docs/superpowers/specs/2026-04-12-nexus-prd-02-security-design.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod error;

pub use error::SecurityError;
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p nexus-security`
Expected: compiles (the error module doesn't exist yet, so this will fail — that's expected; we add it next)

Actually, we need the error module first. Create a minimal placeholder:

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-security/src/error.rs`:

```rust
//! Security error types.

/// Errors from the security subsystem.
#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    /// Placeholder — replaced in Task 4.
    #[error("not yet implemented")]
    NotImplemented,
}
```

Run: `cargo check -p nexus-security`
Expected: compiles successfully.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/nexus-security/
git commit -m "feat(security): scaffold nexus-security crate with workspace wiring"
```

---

### Task 2: Verify workspace builds clean

**Files:** (none created — verification only)

- [ ] **Step 1: Full workspace check**

Run: `cargo check --workspace`
Expected: compiles. No errors from the new crate or the existing crates.

- [ ] **Step 2: Run existing tests**

Run: `cargo nextest run --workspace`
Expected: all PRD 01 tests pass. No regressions.

---

## Phase 2: Display Impl on Capability

### Task 3: Add `Display` impl for `Capability` in nexus-kernel

The `SecurityError::CapabilityDenied` variant wraps a `Capability` and uses it in its error message. `thiserror`'s `#[error("...{0}")]` needs `Display`. The kernel's `Capability` only has `Debug` + `as_str()` currently.

**Files:**
- Modify: `crates/nexus-kernel/src/capability.rs`

- [ ] **Step 1: Write a test for Display**

Add to the `#[cfg(test)] mod tests` block in `crates/nexus-kernel/src/capability.rs`:

```rust
    #[test]
    fn display_matches_as_str() {
        for &cap in Capability::ALL {
            assert_eq!(format!("{cap}"), cap.as_str());
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p nexus-kernel -- display_matches_as_str`
Expected: FAIL — `Capability` doesn't implement `Display`.

- [ ] **Step 3: Implement Display**

Add above the `use std::collections::HashSet;` line in `crates/nexus-kernel/src/capability.rs`:

```rust
impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p nexus-kernel -- display_matches_as_str`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/nexus-kernel/src/capability.rs
git commit -m "feat(kernel): add Display impl for Capability (delegates to as_str)"
```

---

## Phase 3: SecurityError Enum

### Task 4: Define the full SecurityError enum

**Files:**
- Modify: `crates/nexus-security/src/error.rs`

- [ ] **Step 1: Write tests for error Display messages**

Replace the contents of `/mnt/c/Users/baile/dev/nexus/crates/nexus-security/src/error.rs` with:

```rust
//! Security error types.

use nexus_kernel::Capability;
use std::path::PathBuf;

/// Errors from the security subsystem.
#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    /// OS keyring is unavailable (no D-Bus, locked Keychain, etc.).
    #[error("OS keyring unavailable: {reason}\n  {platform_hint}")]
    KeyringUnavailable {
        /// The underlying error from keyring-rs.
        reason: String,
        /// Platform-specific remediation hint.
        platform_hint: String,
    },

    /// Keyring disabled via `NEXUS_NO_KEYRING=1`.
    #[error("keyring disabled (NEXUS_NO_KEYRING=1): credential operations unavailable")]
    KeyringDisabled,

    /// Requested credential not found in keyring.
    #[error("credential not found: {0}")]
    CredentialNotFound(String),

    /// Failed to store a credential in the keyring.
    #[error("failed to store credential: {0}")]
    CredentialStoreFailed(String),

    /// Path traversal attempt detected — resolved path escapes forge root.
    #[error("path traversal denied: {} escapes forge root", .0.display())]
    PathTraversal(PathBuf),

    /// Path contains invalid characters (null bytes, etc.).
    #[error("invalid path: {0}")]
    InvalidPath(String),

    /// A plugin attempted an operation without the required capability.
    #[error("capability denied: {0}")]
    CapabilityDenied(Capability),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyring_unavailable_displays_reason_and_hint() {
        let err = SecurityError::KeyringUnavailable {
            reason: "D-Bus not running".to_string(),
            platform_hint: "Ensure gnome-keyring is running.".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("D-Bus not running"));
        assert!(msg.contains("gnome-keyring"));
    }

    #[test]
    fn keyring_disabled_display() {
        let err = SecurityError::KeyringDisabled;
        let msg = format!("{err}");
        assert!(msg.contains("NEXUS_NO_KEYRING=1"));
    }

    #[test]
    fn credential_not_found_displays_name() {
        let err = SecurityError::CredentialNotFound("ai.anthropic".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("ai.anthropic"));
    }

    #[test]
    fn credential_store_failed_displays_reason() {
        let err = SecurityError::CredentialStoreFailed("permission denied".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("permission denied"));
    }

    #[test]
    fn path_traversal_displays_path() {
        let err = SecurityError::PathTraversal(PathBuf::from("/etc/passwd"));
        let msg = format!("{err}");
        assert!(msg.contains("/etc/passwd"));
        assert!(msg.contains("escapes forge root"));
    }

    #[test]
    fn invalid_path_displays_reason() {
        let err = SecurityError::InvalidPath("contains null byte".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("null byte"));
    }

    #[test]
    fn capability_denied_displays_cap_name() {
        let err = SecurityError::CapabilityDenied(Capability::FsRead);
        let msg = format!("{err}");
        assert!(msg.contains("fs.read"));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo nextest run -p nexus-security`
Expected: all 7 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-security/src/error.rs
git commit -m "feat(security): define SecurityError enum with all M1 variants"
```

---

## Phase 4: Risk Level

### Task 5: Define RiskLevel enum

**Files:**
- Create: `crates/nexus-security/src/risk.rs`
- Modify: `crates/nexus-security/src/lib.rs`

- [ ] **Step 1: Write the risk module with tests**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-security/src/risk.rs`:

```rust
//! Capability risk-level metadata.
//!
//! Risk levels determine install-time prompting: community plugins requesting
//! HIGH-risk capabilities require explicit user approval.

use nexus_kernel::Capability;
use serde::{Deserialize, Serialize};

/// Risk level assigned to a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RiskLevel {
    /// Minimal risk — granted without prompt for all trust levels.
    Low,
    /// Moderate risk — granted without prompt for all trust levels.
    Medium,
    /// Significant risk — requires explicit user approval for community plugins.
    High,
}

/// Returns the risk level for a capability.
///
/// The match is exhaustive over all `Capability` variants. Adding a variant
/// to the kernel will cause a compile error here, forcing the risk mapping
/// to be updated.
#[must_use]
pub fn risk_level(cap: Capability) -> RiskLevel {
    match cap {
        Capability::FsRead
        | Capability::KvRead
        | Capability::KvWrite => RiskLevel::Low,

        Capability::FsWrite
        | Capability::NetHttpLocalhost
        | Capability::DbQuery
        | Capability::DbWrite => RiskLevel::Medium,

        Capability::FsReadExternal
        | Capability::FsWriteExternal
        | Capability::NetHttp
        | Capability::ProcessSpawn
        | Capability::IpcCall => RiskLevel::High,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_capability_has_a_risk_level() {
        for &cap in Capability::ALL {
            // Just call it — if the match is non-exhaustive, this won't compile
            let _level = risk_level(cap);
        }
    }

    #[test]
    fn fs_read_is_low() {
        assert_eq!(risk_level(Capability::FsRead), RiskLevel::Low);
    }

    #[test]
    fn kv_read_is_low() {
        assert_eq!(risk_level(Capability::KvRead), RiskLevel::Low);
    }

    #[test]
    fn kv_write_is_low() {
        assert_eq!(risk_level(Capability::KvWrite), RiskLevel::Low);
    }

    #[test]
    fn fs_write_is_medium() {
        assert_eq!(risk_level(Capability::FsWrite), RiskLevel::Medium);
    }

    #[test]
    fn net_http_localhost_is_medium() {
        assert_eq!(risk_level(Capability::NetHttpLocalhost), RiskLevel::Medium);
    }

    #[test]
    fn db_query_is_medium() {
        assert_eq!(risk_level(Capability::DbQuery), RiskLevel::Medium);
    }

    #[test]
    fn db_write_is_medium() {
        assert_eq!(risk_level(Capability::DbWrite), RiskLevel::Medium);
    }

    #[test]
    fn fs_read_external_is_high() {
        assert_eq!(risk_level(Capability::FsReadExternal), RiskLevel::High);
    }

    #[test]
    fn fs_write_external_is_high() {
        assert_eq!(risk_level(Capability::FsWriteExternal), RiskLevel::High);
    }

    #[test]
    fn net_http_is_high() {
        assert_eq!(risk_level(Capability::NetHttp), RiskLevel::High);
    }

    #[test]
    fn process_spawn_is_high() {
        assert_eq!(risk_level(Capability::ProcessSpawn), RiskLevel::High);
    }

    #[test]
    fn ipc_call_is_high() {
        assert_eq!(risk_level(Capability::IpcCall), RiskLevel::High);
    }

    #[test]
    fn risk_level_is_copy_and_eq() {
        let a = RiskLevel::High;
        let b = a;
        assert_eq!(a, b);
    }
}
```

- [ ] **Step 2: Export from lib.rs**

Replace `/mnt/c/Users/baile/dev/nexus/crates/nexus-security/src/lib.rs` with:

```rust
//! Nexus security: capability risk metadata, credential vault, audit logging,
//! and forge path validation.
//!
//! See `docs/superpowers/specs/2026-04-12-nexus-prd-02-security-design.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod error;
mod risk;

pub use error::SecurityError;
pub use risk::{risk_level, RiskLevel};
```

- [ ] **Step 3: Run tests**

Run: `cargo nextest run -p nexus-security`
Expected: all error tests + all risk tests PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-security/src/risk.rs crates/nexus-security/src/lib.rs
git commit -m "feat(security): add RiskLevel enum and risk_level(Capability) mapping"
```

---

### Task 6: Add RiskLevel::is_high helper and Display impl

**Files:**
- Modify: `crates/nexus-security/src/risk.rs`

- [ ] **Step 1: Write tests**

Add to the `#[cfg(test)] mod tests` block in `crates/nexus-security/src/risk.rs`:

```rust
    #[test]
    fn is_high_returns_true_only_for_high() {
        assert!(RiskLevel::High.is_high());
        assert!(!RiskLevel::Medium.is_high());
        assert!(!RiskLevel::Low.is_high());
    }

    #[test]
    fn display_formats_as_uppercase() {
        assert_eq!(format!("{}", RiskLevel::Low), "LOW");
        assert_eq!(format!("{}", RiskLevel::Medium), "MEDIUM");
        assert_eq!(format!("{}", RiskLevel::High), "HIGH");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo nextest run -p nexus-security -- is_high`
Expected: FAIL — `is_high` method doesn't exist.

- [ ] **Step 3: Implement**

Add to the `RiskLevel` impl block (create one if needed) in `crates/nexus-security/src/risk.rs`, after the enum definition:

```rust
impl RiskLevel {
    /// Returns `true` if this is `RiskLevel::High`.
    #[must_use]
    pub const fn is_high(self) -> bool {
        matches!(self, RiskLevel::High)
    }
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiskLevel::Low => f.write_str("LOW"),
            RiskLevel::Medium => f.write_str("MEDIUM"),
            RiskLevel::High => f.write_str("HIGH"),
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo nextest run -p nexus-security`
Expected: all tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/nexus-security/src/risk.rs
git commit -m "feat(security): add RiskLevel::is_high helper and Display impl"
```

---

## Phase 5: Audit Logging

### Task 7: Define structured audit logging functions

**Files:**
- Create: `crates/nexus-security/src/audit.rs`
- Modify: `crates/nexus-security/src/lib.rs`

- [ ] **Step 1: Write the audit module**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-security/src/audit.rs`:

```rust
//! Structured audit event helpers.
//!
//! Audit events are `tracing` events with structured fields. Output
//! destination (rolling file, stderr, etc.) is configured by the binary
//! crate via `tracing-subscriber` + `tracing-appender`. This module only
//! emits events.
//!
//! All audit events carry `audit = true` as a structured field so
//! downstream subscribers can filter them from general application logs.

use std::path::Path;

/// Log a capability grant event.
pub fn log_capability_granted(plugin_id: &str, capability: &str) {
    tracing::info!(
        audit = true,
        plugin_id,
        capability,
        result = "granted",
        "capability granted"
    );
}

/// Log a capability denial event.
pub fn log_capability_denied(plugin_id: &str, capability: &str) {
    tracing::warn!(
        audit = true,
        plugin_id,
        capability,
        result = "denied",
        "capability denied"
    );
}

/// Log a plugin lifecycle transition (e.g. "loaded", "initialized", "started", "stopped", "crashed").
pub fn log_plugin_lifecycle(plugin_id: &str, transition: &str) {
    tracing::info!(
        audit = true,
        plugin_id,
        transition,
        "plugin lifecycle"
    );
}

/// Log a credential access event. The credential value is never logged.
pub fn log_credential_access(credential_name: &str, action: &str) {
    tracing::info!(
        audit = true,
        credential_name,
        action,
        "credential access"
    );
}

/// Log a path traversal denial.
pub fn log_path_traversal_denied(plugin_id: &str, requested_path: &Path, forge_root: &Path) {
    tracing::warn!(
        audit = true,
        plugin_id,
        requested_path = %requested_path.display(),
        forge_root = %forge_root.display(),
        "path traversal denied"
    );
}
```

- [ ] **Step 2: Export from lib.rs**

Add to `/mnt/c/Users/baile/dev/nexus/crates/nexus-security/src/lib.rs`:

```rust
pub mod audit;
```

(Use `pub mod` not `mod` + `pub use` — callers access as `nexus_security::audit::log_capability_granted(...)`. The module is the namespace.)

The full `lib.rs` should now be:

```rust
//! Nexus security: capability risk metadata, credential vault, audit logging,
//! and forge path validation.
//!
//! See `docs/superpowers/specs/2026-04-12-nexus-prd-02-security-design.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod audit;
mod error;
mod risk;

pub use error::SecurityError;
pub use risk::{risk_level, RiskLevel};
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p nexus-security`
Expected: compiles successfully.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-security/src/audit.rs crates/nexus-security/src/lib.rs
git commit -m "feat(security): add structured audit logging functions via tracing"
```

---

### Task 8: Test audit events are emitted with correct fields

**Files:**
- Modify: `crates/nexus-security/src/audit.rs`

- [ ] **Step 1: Write tests using tracing-subscriber test layer**

Add to the bottom of `crates/nexus-security/src/audit.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::layer::SubscriberExt;

    /// A simple test layer that captures formatted event strings.
    struct CaptureLayer {
        events: Arc<Mutex<Vec<String>>>,
    }

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for CaptureLayer {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            let mut visitor = StringVisitor(String::new());
            event.record(&mut visitor);
            self.events.lock().unwrap().push(visitor.0);
        }
    }

    struct StringVisitor(String);

    impl tracing::field::Visit for StringVisitor {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            use std::fmt::Write;
            let _ = write!(self.0, "{}={:?} ", field.name(), value);
        }

        fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
            use std::fmt::Write;
            let _ = write!(self.0, "{}={} ", field.name(), value);
        }

        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            use std::fmt::Write;
            let _ = write!(self.0, "{}={} ", field.name(), value);
        }
    }

    fn with_captured_events(f: impl FnOnce()) -> Vec<String> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let layer = CaptureLayer {
            events: Arc::clone(&events),
        };
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, f);
        let guard = events.lock().unwrap();
        guard.clone()
    }

    #[test]
    fn capability_granted_emits_audit_event() {
        let events = with_captured_events(|| {
            log_capability_granted("com.example.test", "fs.read");
        });
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert!(event.contains("audit=true"), "missing audit field: {event}");
        assert!(event.contains("plugin_id=com.example.test"), "missing plugin_id: {event}");
        assert!(event.contains("capability=fs.read"), "missing capability: {event}");
        assert!(event.contains("result=granted"), "missing result: {event}");
    }

    #[test]
    fn capability_denied_emits_audit_event() {
        let events = with_captured_events(|| {
            log_capability_denied("com.example.test", "net.http");
        });
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert!(event.contains("audit=true"), "missing audit field: {event}");
        assert!(event.contains("result=denied"), "missing result: {event}");
    }

    #[test]
    fn plugin_lifecycle_emits_audit_event() {
        let events = with_captured_events(|| {
            log_plugin_lifecycle("com.example.test", "started");
        });
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert!(event.contains("audit=true"), "missing audit field: {event}");
        assert!(event.contains("transition=started"), "missing transition: {event}");
    }

    #[test]
    fn credential_access_emits_audit_event() {
        let events = with_captured_events(|| {
            log_credential_access("ai.anthropic", "retrieve");
        });
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert!(event.contains("audit=true"), "missing audit field: {event}");
        assert!(event.contains("credential_name=ai.anthropic"), "missing credential_name: {event}");
        assert!(event.contains("action=retrieve"), "missing action: {event}");
    }

    #[test]
    fn path_traversal_denied_emits_audit_event() {
        let events = with_captured_events(|| {
            log_path_traversal_denied(
                "com.example.test",
                Path::new("/etc/passwd"),
                Path::new("/home/user/forge"),
            );
        });
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert!(event.contains("audit=true"), "missing audit field: {event}");
        assert!(event.contains("plugin_id=com.example.test"), "missing plugin_id: {event}");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo nextest run -p nexus-security -- audit`
Expected: all 5 audit tests PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-security/src/audit.rs
git commit -m "test(security): add audit event emission tests with tracing capture layer"
```

---

## Phase 6: Forge Path Validation

### Task 9: Implement ForgePathValidator

**Files:**
- Create: `crates/nexus-security/src/path.rs`
- Modify: `crates/nexus-security/src/lib.rs`

- [ ] **Step 1: Write the path module**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-security/src/path.rs`:

```rust
//! Forge-root path validation and symlink enforcement.
//!
//! All plugin file operations pass through `ForgePathValidator::validate`
//! to ensure the resolved path stays within the forge root. Symlinks are
//! followed only if the final canonical path is still inside the root.

use std::path::{Path, PathBuf};

use crate::SecurityError;

/// Validates that file paths resolve within a forge root directory.
///
/// Constructed once per forge, canonicalizes the root at creation time.
/// Thread-safe (immutable after construction).
#[derive(Debug, Clone)]
pub struct ForgePathValidator {
    forge_root: PathBuf,
}

impl ForgePathValidator {
    /// Create a new validator. Canonicalizes `forge_root` immediately.
    ///
    /// # Errors
    /// Returns `SecurityError::InvalidPath` if `forge_root` does not exist
    /// or cannot be canonicalized.
    pub fn new(forge_root: &Path) -> Result<Self, SecurityError> {
        let canonical = forge_root.canonicalize().map_err(|e| {
            SecurityError::InvalidPath(format!(
                "forge root '{}' cannot be canonicalized: {e}",
                forge_root.display()
            ))
        })?;
        Ok(Self {
            forge_root: canonical,
        })
    }

    /// The canonical forge root path.
    #[must_use]
    pub fn forge_root(&self) -> &Path {
        &self.forge_root
    }

    /// Validate a requested path. Returns the canonical resolved path if it
    /// is within the forge root.
    ///
    /// # Behavior
    /// 1. Rejects paths containing null bytes.
    /// 2. Strips leading `/` (absolute paths treated as relative to forge root).
    /// 3. Normalizes `.` and `..` components, rejecting `..` past the root.
    /// 4. Joins with forge root and canonicalizes (follows symlinks).
    /// 5. Verifies the canonical path starts with the canonical forge root.
    ///
    /// # Errors
    /// - `SecurityError::InvalidPath` for null bytes or non-existent paths.
    /// - `SecurityError::PathTraversal` if the resolved path escapes the root.
    pub fn validate(&self, requested: &Path) -> Result<PathBuf, SecurityError> {
        let requested_str = requested.to_string_lossy();

        // 1. Reject null bytes
        if requested_str.contains('\0') {
            return Err(SecurityError::InvalidPath(
                "path contains null byte".to_string(),
            ));
        }

        // 2. Normalize path components
        let normalized = self.normalize(requested)?;

        // 3. Join with forge root
        let joined = self.forge_root.join(&normalized);

        // 4. Canonicalize (resolves symlinks)
        let canonical = joined.canonicalize().map_err(|e| {
            SecurityError::InvalidPath(format!(
                "path '{}' cannot be resolved: {e}",
                requested.display()
            ))
        })?;

        // 5. Verify within forge root
        if !canonical.starts_with(&self.forge_root) {
            return Err(SecurityError::PathTraversal(canonical));
        }

        Ok(canonical)
    }

    /// Normalize path components: collapse `.`, reject `..` that escapes root.
    /// Strips leading `/` so absolute paths are treated as relative.
    fn normalize(&self, path: &Path) -> Result<PathBuf, SecurityError> {
        let mut components = Vec::new();

        for component in path.components() {
            match component {
                std::path::Component::Normal(c) => {
                    components.push(c);
                }
                std::path::Component::ParentDir => {
                    if components.is_empty() {
                        return Err(SecurityError::PathTraversal(path.to_path_buf()));
                    }
                    components.pop();
                }
                std::path::Component::CurDir | std::path::Component::RootDir => {
                    // Skip `.` and leading `/`
                }
                std::path::Component::Prefix(_) => {
                    // Windows prefix — treat as no-op for forge-relative paths
                }
            }
        }

        if components.is_empty() {
            Ok(PathBuf::from("."))
        } else {
            Ok(components.iter().collect())
        }
    }
}
```

- [ ] **Step 2: Export from lib.rs**

Update `/mnt/c/Users/baile/dev/nexus/crates/nexus-security/src/lib.rs` to:

```rust
//! Nexus security: capability risk metadata, credential vault, audit logging,
//! and forge path validation.
//!
//! See `docs/superpowers/specs/2026-04-12-nexus-prd-02-security-design.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod audit;
mod error;
mod path;
mod risk;

pub use error::SecurityError;
pub use path::ForgePathValidator;
pub use risk::{risk_level, RiskLevel};
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p nexus-security`
Expected: compiles successfully.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-security/src/path.rs crates/nexus-security/src/lib.rs
git commit -m "feat(security): add ForgePathValidator with symlink-safe path resolution"
```

---

### Task 10: Test ForgePathValidator edge cases

**Files:**
- Modify: `crates/nexus-security/src/path.rs`

- [ ] **Step 1: Write tests**

Add to the bottom of `crates/nexus-security/src/path.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn new_fails_on_nonexistent_root() {
        let result = ForgePathValidator::new(Path::new("/nonexistent/path/abc123"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SecurityError::InvalidPath(_)));
    }

    #[test]
    fn forge_root_returns_canonical_path() {
        let dir = tempfile::tempdir().unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();
        assert_eq!(validator.forge_root(), dir.path().canonicalize().unwrap());
    }

    #[test]
    fn valid_file_resolves() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("test.txt"), "hello").unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        let result = validator.validate(Path::new("test.txt"));
        assert!(result.is_ok());
        let resolved = result.unwrap();
        assert!(resolved.starts_with(validator.forge_root()));
    }

    #[test]
    fn valid_nested_file_resolves() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sub/dir")).unwrap();
        fs::write(dir.path().join("sub/dir/file.md"), "content").unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        let result = validator.validate(Path::new("sub/dir/file.md"));
        assert!(result.is_ok());
    }

    #[test]
    fn dot_dot_traversal_past_root_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        let result = validator.validate(Path::new("../../../etc/passwd"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SecurityError::PathTraversal(_)));
    }

    #[test]
    fn dot_dot_within_root_is_allowed() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("a/b")).unwrap();
        fs::write(dir.path().join("a/test.txt"), "hello").unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        // a/b/../test.txt normalizes to a/test.txt — still in root
        let result = validator.validate(Path::new("a/b/../test.txt"));
        assert!(result.is_ok());
    }

    #[test]
    fn null_byte_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        let result = validator.validate(Path::new("test\0.txt"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SecurityError::InvalidPath(_)));
    }

    #[test]
    fn absolute_path_treated_as_relative() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("file.txt"), "hello").unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        // /file.txt should be treated as file.txt relative to forge root
        let result = validator.validate(Path::new("/file.txt"));
        assert!(result.is_ok());
        assert!(result.unwrap().starts_with(validator.forge_root()));
    }

    #[test]
    fn empty_path_resolves_to_forge_root() {
        let dir = tempfile::tempdir().unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        let result = validator.validate(Path::new(""));
        // Empty path joined with root = root itself, which is valid
        assert!(result.is_ok());
    }

    #[test]
    fn dot_resolves_to_forge_root() {
        let dir = tempfile::tempdir().unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        let result = validator.validate(Path::new("."));
        assert!(result.is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_within_root_is_allowed() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("real.txt"), "hello").unwrap();
        std::os::unix::fs::symlink(
            dir.path().join("real.txt"),
            dir.path().join("link.txt"),
        )
        .unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        let result = validator.validate(Path::new("link.txt"));
        assert!(result.is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_outside_root_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.txt"), "secret").unwrap();

        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            dir.path().join("escape.txt"),
        )
        .unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        let result = validator.validate(Path::new("escape.txt"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SecurityError::PathTraversal(_)));
    }

    #[test]
    fn nonexistent_file_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let validator = ForgePathValidator::new(dir.path()).unwrap();

        let result = validator.validate(Path::new("does_not_exist.txt"));
        assert!(result.is_err());
        // canonicalize fails on nonexistent paths
        assert!(matches!(result.unwrap_err(), SecurityError::InvalidPath(_)));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo nextest run -p nexus-security -- path`
Expected: all path tests PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-security/src/path.rs
git commit -m "test(security): add ForgePathValidator edge-case tests (traversal, symlinks, null bytes)"
```

---

## Phase 7: Credential Vault

### Task 11: Implement CredentialVault struct and disabled-mode behavior

**Files:**
- Create: `crates/nexus-security/src/credential.rs`
- Modify: `crates/nexus-security/src/lib.rs`

- [ ] **Step 1: Write the credential module**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-security/src/credential.rs`:

```rust
//! Generic credential vault over the OS keyring.
//!
//! Thin wrapper around `keyring-rs`. Credentials are stored as key-value
//! pairs with a dot-namespaced naming convention (e.g., `"ai.anthropic"`).
//!
//! Hard-fail policy (ADR 0009): `available()` is called at startup. If
//! the keyring is unavailable and `NEXUS_NO_KEYRING` is not set, Nexus
//! refuses to start. If `NEXUS_NO_KEYRING=1`, credential operations
//! return `SecurityError::KeyringDisabled`.

use crate::SecurityError;

/// Service name used for all keyring entries.
const SERVICE_NAME: &str = "nexus";

/// Generic credential vault backed by the OS keyring.
#[derive(Debug)]
pub struct CredentialVault {
    disabled: bool,
}

impl CredentialVault {
    /// Create a new credential vault.
    ///
    /// Checks the `NEXUS_NO_KEYRING` environment variable. If set to `"1"`,
    /// the vault operates in disabled mode: `available()` returns `Ok(())`
    /// but all credential operations return `SecurityError::KeyringDisabled`.
    #[must_use]
    pub fn new() -> Self {
        let disabled = std::env::var("NEXUS_NO_KEYRING")
            .map(|v| v == "1")
            .unwrap_or(false);
        Self { disabled }
    }

    /// Check whether the OS keyring is accessible.
    ///
    /// In disabled mode (`NEXUS_NO_KEYRING=1`), returns `Ok(())` — the
    /// startup check passes, but individual operations will fail with
    /// `KeyringDisabled`.
    ///
    /// # Errors
    /// Returns `SecurityError::KeyringUnavailable` with a platform-specific
    /// hint if the keyring cannot be accessed.
    pub fn available(&self) -> Result<(), SecurityError> {
        if self.disabled {
            return Ok(());
        }

        // Probe the keyring by attempting to get a non-existent entry.
        // keyring-rs returns NoEntry (not a platform error) if the keyring
        // works but the entry doesn't exist — that's a success for our probe.
        let entry = keyring::Entry::new(SERVICE_NAME, "__nexus_probe__")
            .map_err(|e| platform_error(e.to_string()))?;

        match entry.get_password() {
            Ok(_) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()), // keyring works, entry just missing
            Err(e) => Err(platform_error(e.to_string())),
        }
    }

    /// Store a credential in the OS keyring.
    ///
    /// # Errors
    /// - `SecurityError::KeyringDisabled` if `NEXUS_NO_KEYRING=1`.
    /// - `SecurityError::CredentialStoreFailed` if the keyring operation fails.
    pub fn store(&self, name: &str, value: &str) -> Result<(), SecurityError> {
        if self.disabled {
            return Err(SecurityError::KeyringDisabled);
        }

        crate::audit::log_credential_access(name, "store");

        let entry = keyring::Entry::new(SERVICE_NAME, name)
            .map_err(|e| SecurityError::CredentialStoreFailed(e.to_string()))?;

        entry
            .set_password(value)
            .map_err(|e| SecurityError::CredentialStoreFailed(e.to_string()))
    }

    /// Retrieve a credential from the OS keyring.
    ///
    /// # Errors
    /// - `SecurityError::KeyringDisabled` if `NEXUS_NO_KEYRING=1`.
    /// - `SecurityError::CredentialNotFound` if the credential doesn't exist.
    pub fn retrieve(&self, name: &str) -> Result<String, SecurityError> {
        if self.disabled {
            return Err(SecurityError::KeyringDisabled);
        }

        crate::audit::log_credential_access(name, "retrieve");

        let entry = keyring::Entry::new(SERVICE_NAME, name)
            .map_err(|e| SecurityError::CredentialNotFound(e.to_string()))?;

        entry.get_password().map_err(|e| match e {
            keyring::Error::NoEntry => SecurityError::CredentialNotFound(name.to_string()),
            other => SecurityError::CredentialNotFound(other.to_string()),
        })
    }

    /// Delete a credential from the OS keyring.
    ///
    /// # Errors
    /// - `SecurityError::KeyringDisabled` if `NEXUS_NO_KEYRING=1`.
    /// - `SecurityError::CredentialNotFound` if the credential doesn't exist.
    pub fn delete(&self, name: &str) -> Result<(), SecurityError> {
        if self.disabled {
            return Err(SecurityError::KeyringDisabled);
        }

        crate::audit::log_credential_access(name, "delete");

        let entry = keyring::Entry::new(SERVICE_NAME, name)
            .map_err(|e| SecurityError::CredentialNotFound(e.to_string()))?;

        entry.delete_credential().map_err(|e| match e {
            keyring::Error::NoEntry => SecurityError::CredentialNotFound(name.to_string()),
            other => SecurityError::CredentialNotFound(other.to_string()),
        })
    }

    /// Whether the vault is in disabled mode (`NEXUS_NO_KEYRING=1`).
    #[must_use]
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }
}

impl Default for CredentialVault {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a `KeyringUnavailable` error with a platform-specific hint.
fn platform_error(reason: String) -> SecurityError {
    let platform_hint = if cfg!(target_os = "linux") {
        "On Linux, ensure D-Bus and a Secret Service provider (e.g., gnome-keyring or KWallet) are running.".to_string()
    } else if cfg!(target_os = "macos") {
        "On macOS, ensure Keychain Access is unlocked.".to_string()
    } else if cfg!(target_os = "windows") {
        "On Windows, ensure Credential Manager is accessible.".to_string()
    } else {
        "Ensure your platform's credential storage is configured and accessible.".to_string()
    };

    SecurityError::KeyringUnavailable {
        reason,
        platform_hint,
    }
}
```

- [ ] **Step 2: Export from lib.rs**

Update `/mnt/c/Users/baile/dev/nexus/crates/nexus-security/src/lib.rs` to:

```rust
//! Nexus security: capability risk metadata, credential vault, audit logging,
//! and forge path validation.
//!
//! See `docs/superpowers/specs/2026-04-12-nexus-prd-02-security-design.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod audit;
mod credential;
mod error;
mod path;
mod risk;

pub use credential::CredentialVault;
pub use error::SecurityError;
pub use path::ForgePathValidator;
pub use risk::{risk_level, RiskLevel};
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p nexus-security`
Expected: compiles successfully.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-security/src/credential.rs crates/nexus-security/src/lib.rs
git commit -m "feat(security): add CredentialVault with keyring-rs backend and disabled mode"
```

---

### Task 12: Test CredentialVault disabled mode (no keyring required)

**Files:**
- Modify: `crates/nexus-security/src/credential.rs`

- [ ] **Step 1: Write disabled-mode tests**

Add to the bottom of `crates/nexus-security/src/credential.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a vault in disabled mode by temporarily setting the env var.
    fn disabled_vault() -> CredentialVault {
        // Safety: tests run single-threaded for env manipulation.
        // We restore the var after creating the vault.
        std::env::set_var("NEXUS_NO_KEYRING", "1");
        let vault = CredentialVault::new();
        std::env::remove_var("NEXUS_NO_KEYRING");
        vault
    }

    #[test]
    fn disabled_vault_is_disabled() {
        let vault = disabled_vault();
        assert!(vault.is_disabled());
    }

    #[test]
    fn disabled_available_returns_ok() {
        let vault = disabled_vault();
        assert!(vault.available().is_ok());
    }

    #[test]
    fn disabled_store_returns_keyring_disabled() {
        let vault = disabled_vault();
        let err = vault.store("ai.test", "secret123").unwrap_err();
        assert!(matches!(err, SecurityError::KeyringDisabled));
    }

    #[test]
    fn disabled_retrieve_returns_keyring_disabled() {
        let vault = disabled_vault();
        let err = vault.retrieve("ai.test").unwrap_err();
        assert!(matches!(err, SecurityError::KeyringDisabled));
    }

    #[test]
    fn disabled_delete_returns_keyring_disabled() {
        let vault = disabled_vault();
        let err = vault.delete("ai.test").unwrap_err();
        assert!(matches!(err, SecurityError::KeyringDisabled));
    }

    #[test]
    fn default_vault_not_disabled_when_env_unset() {
        // Ensure the env var is not set
        std::env::remove_var("NEXUS_NO_KEYRING");
        let vault = CredentialVault::new();
        assert!(!vault.is_disabled());
    }

    #[test]
    fn env_value_other_than_1_is_not_disabled() {
        std::env::set_var("NEXUS_NO_KEYRING", "true");
        let vault = CredentialVault::new();
        std::env::remove_var("NEXUS_NO_KEYRING");
        assert!(!vault.is_disabled(), "only '1' should disable the keyring");
    }

    #[test]
    fn platform_error_contains_platform_hint() {
        let err = platform_error("test reason".to_string());
        match err {
            SecurityError::KeyringUnavailable {
                reason,
                platform_hint,
            } => {
                assert_eq!(reason, "test reason");
                assert!(!platform_hint.is_empty());
            }
            other => panic!("expected KeyringUnavailable, got {other:?}"),
        }
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo nextest run -p nexus-security -- credential`
Expected: all 8 credential tests PASS. These tests do NOT touch the real keyring.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-security/src/credential.rs
git commit -m "test(security): add CredentialVault disabled-mode tests (no keyring required)"
```

---

### Task 13: PRD 02 smoke test — public API surface and integration check

**Files:**
- Create: `tests/prd-02-smoke.rs`

- [ ] **Step 1: Write smoke test**

Write `/mnt/c/Users/baile/dev/nexus/tests/prd-02-smoke.rs`:

```rust
//! PRD 02 §12 smoke test: verifies the `nexus-security` public API surface
//! compiles, types are accessible, and basic behavior is correct.
//!
//! This test does NOT exercise the real OS keyring — it only tests the
//! disabled-mode path and non-keyring modules.

use nexus_kernel::Capability;
use nexus_security::{
    audit, risk_level, CredentialVault, ForgePathValidator, RiskLevel, SecurityError,
};
use std::path::Path;

#[test]
fn risk_level_covers_all_capabilities() {
    for &cap in Capability::ALL {
        let level = risk_level(cap);
        // Every capability must map to a valid risk level
        assert!(
            matches!(level, RiskLevel::Low | RiskLevel::Medium | RiskLevel::High),
            "unexpected risk level for {cap}: {level}"
        );
    }
}

#[test]
fn risk_level_high_caps_match_spec() {
    let high_caps = [
        Capability::FsReadExternal,
        Capability::FsWriteExternal,
        Capability::NetHttp,
        Capability::ProcessSpawn,
        Capability::IpcCall,
    ];
    for cap in high_caps {
        assert!(
            risk_level(cap).is_high(),
            "{cap} should be HIGH risk"
        );
    }
}

#[test]
fn credential_vault_disabled_mode() {
    std::env::set_var("NEXUS_NO_KEYRING", "1");
    let vault = CredentialVault::new();
    std::env::remove_var("NEXUS_NO_KEYRING");

    assert!(vault.is_disabled());
    assert!(vault.available().is_ok());
    assert!(matches!(
        vault.store("test", "val").unwrap_err(),
        SecurityError::KeyringDisabled
    ));
    assert!(matches!(
        vault.retrieve("test").unwrap_err(),
        SecurityError::KeyringDisabled
    ));
    assert!(matches!(
        vault.delete("test").unwrap_err(),
        SecurityError::KeyringDisabled
    ));
}

#[test]
fn forge_path_validator_blocks_traversal() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("ok.txt"), "safe").unwrap();

    let validator = ForgePathValidator::new(dir.path()).unwrap();

    // Valid path succeeds
    assert!(validator.validate(Path::new("ok.txt")).is_ok());

    // Traversal is blocked
    assert!(validator.validate(Path::new("../../../etc/passwd")).is_err());
}

#[test]
fn audit_functions_callable() {
    // Just verify these compile and don't panic when called without a subscriber.
    // In production, a tracing subscriber captures the output.
    audit::log_capability_granted("com.test.smoke", "fs.read");
    audit::log_capability_denied("com.test.smoke", "net.http");
    audit::log_plugin_lifecycle("com.test.smoke", "started");
    audit::log_credential_access("ai.test", "retrieve");
    audit::log_path_traversal_denied(
        "com.test.smoke",
        Path::new("../escape"),
        Path::new("/forge/root"),
    );
}

#[test]
fn security_error_display_is_human_readable() {
    let err = SecurityError::CapabilityDenied(Capability::NetHttp);
    let msg = format!("{err}");
    assert!(msg.contains("net.http"), "error should show capability name: {msg}");

    let err = SecurityError::PathTraversal(std::path::PathBuf::from("/bad/path"));
    let msg = format!("{err}");
    assert!(msg.contains("escapes forge root"), "error should mention traversal: {msg}");
}

#[test]
fn public_type_surface_is_accessible() {
    // Verify all public types are importable and constructible where applicable
    let _level: RiskLevel = RiskLevel::High;
    let _level: RiskLevel = RiskLevel::Medium;
    let _level: RiskLevel = RiskLevel::Low;

    // CredentialVault::new() works
    std::env::remove_var("NEXUS_NO_KEYRING");
    let _vault = CredentialVault::new();

    // ForgePathValidator requires a real directory
    let dir = tempfile::tempdir().unwrap();
    let _validator = ForgePathValidator::new(dir.path()).unwrap();

    // SecurityError variants are constructible
    let _err = SecurityError::KeyringDisabled;
    let _err = SecurityError::CredentialNotFound("test".to_string());
    let _err = SecurityError::InvalidPath("bad".to_string());
    let _err = SecurityError::CapabilityDenied(Capability::FsRead);
}
```

- [ ] **Step 2: Run the smoke test**

Run: `cargo nextest run --test prd-02-smoke`
Expected: all 7 tests PASS.

- [ ] **Step 3: Run full workspace test suite**

Run: `cargo nextest run --workspace`
Expected: all PRD 01 + PRD 02 tests PASS. No regressions.

- [ ] **Step 4: Run clippy on the new crate**

Run: `cargo clippy -p nexus-security -- -D warnings`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add tests/prd-02-smoke.rs
git commit -m "test(security): add PRD 02 smoke test covering public API surface and integration"
```

---

## Post-Completion Checklist

After all tasks are done, verify:

1. `cargo nextest run --workspace` — all tests pass
2. `cargo clippy --workspace -- -D warnings` — no warnings
3. `cargo doc -p nexus-security --no-deps` — docs build without warnings
4. The `nexus-security` crate exports exactly: `SecurityError`, `RiskLevel`, `risk_level`, `CredentialVault`, `ForgePathValidator`, `audit` module
5. No `unwrap()` or `expect()` in production code (only in tests)
6. All public items have doc comments (`#![deny(missing_docs)]` enforces this)
