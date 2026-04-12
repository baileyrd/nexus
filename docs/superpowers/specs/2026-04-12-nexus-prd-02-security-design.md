# Nexus PRD 02 — Security Model (M1-Slimmed) Design Spec

**Version:** 0.1
**Date:** 2026-04-12
**Status:** Approved (brainstorming session output)
**Scope:** Implementation contract for the `nexus-security` crate within M1. Covers only the M1-slimmed subset of PRD 02 as defined in the M1 Foundation Spec §9.

**Parent docs:**
- [`2026-04-11-nexus-roadmap-design.md`](./2026-04-11-nexus-roadmap-design.md)
- [`2026-04-11-nexus-m1-foundation-spec.md`](./2026-04-11-nexus-m1-foundation-spec.md) — §9 (Security Model, slimmed)
- [`PRDs/02-security-model.md`](../../../PRDs/02-security-model.md) — full PRD (many sections deferred)

**Relevant ADRs:**
- [ADR 0002](../../adr/0002-hierarchical-capability-strings.md) — hierarchical dot-namespaced capability strings
- [ADR 0009](../../adr/0009-keyring-hard-fail-policy.md) — keyring hard-fail policy
- [ADR 0010](../../adr/0010-no-plugin-signing-in-m1.md) — no plugin signing in M1

---

## 1. Scope

### In scope (M1)

1. **Capability risk-level metadata** — `RiskLevel` enum and a `risk_level(Capability) -> RiskLevel` mapping function, per M1 spec §4.
2. **Credential vault** — generic key-value credential storage over `keyring-rs` with hard-fail startup policy (ADR 0009) and `NEXUS_NO_KEYRING=1` escape hatch.
3. **Structured audit logging** — capability grants/denials, plugin lifecycle transitions, and credential access events logged via `tracing` structured fields. No custom log store.
4. **Forge path validation** — symlink-safe path normalization and traversal prevention, ensuring all plugin file operations stay within `forge_root`.
5. **Security error types** — `SecurityError` enum covering all failure modes.

### Out of scope (cut from M1)

- Plugin signing / Ed25519 verification (ADR 0010, deferred to v0.2)
- WASM sandbox (lives in `nexus-plugins`, PRD 04)
- Sync/replication encryption (CRDT E2E, HMAC — cut from v0.1)
- TLS certificate pinning / network security (no network ops in M1)
- AI safety / prompt injection detection (M4)
- Hash-chained audit logs, merkle tamper detection, JSONL export, log rotation/compression
- Privacy dashboard, security alerts UI
- Incident response automation, vulnerability reporting workflows
- Fuzzing harness

---

## 2. Crate Structure

### Dependencies

```
nexus-security ──> nexus-kernel (for Capability enum)
               ──> nexus-types
```

Per M1 spec §2 dependency graph. `nexus-security` does NOT depend on `nexus-plugins`.

### Third-party dependencies

| Crate | Version | Purpose |
|---|---|---|
| `keyring` | 3.x | OS keychain integration (Linux secret-service, macOS Keychain, Windows Credential Manager) |
| `tracing` | 0.1 | Structured audit event emission |
| `thiserror` | 2.0 | Error type derivation |
| `serde` | 1.0 | Serialization for `RiskLevel` |

Note: `keyring` 3.x (not 2.3 as listed in M1 spec — verify latest stable at implementation time and update workspace deps accordingly).

### File layout

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

---

## 3. Module Designs

### 3.1 `error.rs` — Security Error Types

```rust
use nexus_kernel::Capability;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    #[error("OS keyring unavailable: {reason}\n  {platform_hint}")]
    KeyringUnavailable {
        reason: String,
        platform_hint: String,
    },

    #[error("keyring disabled (NEXUS_NO_KEYRING=1): credential operations unavailable")]
    KeyringDisabled,

    #[error("credential not found: {0}")]
    CredentialNotFound(String),

    #[error("failed to store credential: {0}")]
    CredentialStoreFailed(String),

    #[error("path traversal denied: {0} escapes forge root")]
    PathTraversal(PathBuf),

    #[error("invalid path: {0}")]
    InvalidPath(String),

    #[error("capability denied: {0}")]
    CapabilityDenied(Capability),
}
```

`Capability` must implement `Display` (it already has `as_str()`, so the `#[error]` format will use that or a `Display` impl added to the kernel).

### 3.2 `risk.rs` — Capability Risk Metadata

```rust
use nexus_kernel::Capability;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

/// Returns the risk level for a given capability.
///
/// Risk levels determine install-time prompting behavior:
/// - Low/Medium: granted without prompt for all trust levels
/// - High: requires explicit user approval for community plugins
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
```

The match must be exhaustive over all `Capability` variants. Adding a variant to the kernel forces a compile error here, which is the desired behavior.

### 3.3 `credential.rs` — Generic Credential Vault

```rust
pub struct CredentialVault {
    service_name: &'static str,
    disabled: bool, // true when NEXUS_NO_KEYRING=1
}
```

**Public API:**

- `CredentialVault::new() -> Self` — constructs with service name `"nexus"`. Checks `NEXUS_NO_KEYRING` env var.
- `available(&self) -> Result<(), SecurityError>` — probes the keyring by attempting a no-op operation. If `disabled`, returns `Ok(())` (the check passes; individual ops will fail later). If the keyring is genuinely unavailable, returns `SecurityError::KeyringUnavailable` with a platform-specific hint message:
  - Linux: "Ensure D-Bus and a Secret Service provider (e.g., gnome-keyring or KWallet) are running."
  - macOS: "Ensure Keychain Access is unlocked."
  - Windows: "Ensure Credential Manager is accessible."
- `store(&self, name: &str, value: &str) -> Result<(), SecurityError>` — stores via `keyring::Entry::new("nexus", name).set_password(value)`. Returns `KeyringDisabled` if disabled.
- `retrieve(&self, name: &str) -> Result<String, SecurityError>` — retrieves via `keyring::Entry::new("nexus", name).get_password()`. Returns `CredentialNotFound` if not present, `KeyringDisabled` if disabled.
- `delete(&self, name: &str) -> Result<(), SecurityError>` — deletes via `keyring::Entry::new("nexus", name).delete_credential()`. Returns `CredentialNotFound` if not present, `KeyringDisabled` if disabled.

**Key naming convention:** dot-namespaced, matching the capability string style. Examples:
- `"ai.anthropic"` — Anthropic API key
- `"ai.openai"` — OpenAI API key
- `"mcp.server-name"` — MCP server credential

Audit events are emitted on every credential operation (store/retrieve/delete) via the `audit` module. The credential value itself is never logged.

### 3.4 `audit.rs` — Structured Audit Logging

No custom log store. Audit events are `tracing` events with structured fields. The output destination (rolling file, stderr, etc.) is configured by the binary crate (`nexus-cli`) via `tracing-subscriber` + `tracing-appender`.

**Public functions:**

```rust
pub fn log_capability_granted(plugin_id: &str, capability: &str);
pub fn log_capability_denied(plugin_id: &str, capability: &str);
pub fn log_plugin_lifecycle(plugin_id: &str, transition: &str);
pub fn log_credential_access(credential_name: &str, action: &str);
pub fn log_path_traversal_denied(plugin_id: &str, requested_path: &Path, forge_root: &Path);
```

Each function emits a single `tracing::info!` (or `tracing::warn!` for denials) with structured fields:

```rust
// Example implementation sketch
pub fn log_capability_denied(plugin_id: &str, capability: &str) {
    tracing::warn!(
        audit = true,
        plugin_id,
        capability,
        result = "denied",
        "capability check denied"
    );
}
```

The `audit = true` field allows downstream subscribers to filter audit events from general application logs.

**Log levels per M1 spec §9.2:**
- `info`: capability grants/denials, plugin lifecycle transitions
- `debug`: file system access (off by default)
- `warn`: security-relevant denials (path traversal, capability denial)

### 3.5 `path.rs` — Forge Path Validation

```rust
pub struct ForgePathValidator {
    forge_root: PathBuf,
}
```

**Public API:**

- `ForgePathValidator::new(forge_root: PathBuf) -> Result<Self, SecurityError>` — canonicalizes `forge_root` at construction. Fails if the root doesn't exist.
- `validate(&self, requested: &Path) -> Result<PathBuf, SecurityError>` — the core validation function:
  1. Reject null bytes → `SecurityError::InvalidPath`
  2. Normalize path components (collapse `.`, reject `..` that escapes root)
  3. Join with `forge_root`
  4. Canonicalize (resolve symlinks)
  5. Verify canonical path starts with canonical `forge_root` → `SecurityError::PathTraversal` if not
  6. Return the validated canonical path

**Symlink policy:**
- Symlinks within forge root: followed safely (resolved path still within root)
- Symlinks pointing outside forge root: rejected with `PathTraversal` error
- All symlink resolutions logged at `debug` level via the audit module

**Edge cases handled:**
- Null bytes in path
- `..` traversal past root
- Symlink chains pointing outside root
- Absolute paths (treated as relative to forge root — leading `/` stripped)
- Empty path (returns forge root itself)

---

## 4. Integration Points

### 4.1 Kernel → Security

The kernel's `PluginContext` implementation (PRD 04, not this crate) will call:
- `risk::risk_level()` during plugin install to determine prompting behavior
- `audit::log_capability_granted/denied()` inside capability enforcement
- `path::ForgePathValidator::validate()` before all file I/O operations

### 4.2 CLI → Security

The CLI binary (`nexus-cli`, PRD 05) will call:
- `CredentialVault::available()` at startup for the hard-fail check
- `CredentialVault::store/retrieve/delete()` for `nexus forge config` credential commands
- Wire up `tracing-appender` for rolling daily log files at `<forge>/.nexus/logs/nexus-YYYY-MM-DD.log`

### 4.3 Plugins → Security (indirect)

Plugins never call `nexus-security` directly. All security enforcement is mediated by the kernel's `PluginContext` impl, which holds a `ForgePathValidator` and calls audit functions internally.

---

## 5. Testing Strategy

### Unit tests (in-crate)

| Module | Tests |
|---|---|
| `risk` | Exhaustive: every `Capability::ALL` variant maps to a `RiskLevel`. Specific assertions on known HIGH-risk caps. |
| `credential` | `NEXUS_NO_KEYRING=1` behavior: `available()` returns Ok, ops return `KeyringDisabled`. Platform keyring tests gated behind `#[cfg(not(ci))]` or an integration test flag. |
| `audit` | Verify tracing events are emitted with correct fields using `tracing-test` or `tracing-subscriber`'s test layer. |
| `path` | Null byte rejection, `..` traversal, symlink-outside-root rejection, absolute path normalization, empty path, normal valid paths. Uses `tempdir` for filesystem fixtures. |
| `error` | Display format assertions for each variant. |

### Acceptance tests (`tests/acceptance/prd-02-security/`)

Per M1 spec §11.1 — these map to the slimmed acceptance criteria:
1. Capability risk levels cover all variants exhaustively
2. Keyring hard-fail: calling `available()` with a broken keyring returns the expected error with platform hint
3. Keyring disabled mode: `NEXUS_NO_KEYRING=1` makes ops return `KeyringDisabled`
4. Path traversal impossible: forge root boundary holds under adversarial paths
5. Audit events emitted for capability grant/deny with correct structured fields

### Integration test seam (I4 in M1 spec)

The I4 integration test (`capability_enforcement`) exercises the full path:
plugin requests capability → kernel checks → `nexus-security` audit logs the result → tracing event captured. This test is written when the kernel's `PluginContext` impl is wired up (PRD 04), not during PRD 02 work.

---

## 6. Non-Goals and Deferred Items

These items exist in PRD 02 but are explicitly deferred. Listed here so future implementers know they were considered and cut, not missed:

| Item | PRD 02 Section | Deferred To |
|---|---|---|
| Plugin signing (Ed25519) | §5 | v0.2 (ADR 0010) |
| Hash-chained audit logs | §11.4 | v0.2 |
| Audit log JSONL export | §11.2–11.3 | v0.2 |
| Audit log rotation/compression | §11.3 | v0.2 |
| TLS certificate pinning | §7.2 | M4 (when network ops exist) |
| AI prompt injection detection | §9 | M4 |
| Sync E2E encryption | §10 | Cut from v0.1 |
| Privacy dashboard | §16 | M2+ |
| Security review workflows | §12 | v0.2 |
| Fuzzing harness | §15.1 | v0.2 |
| Session tokens | §6.1 | M4 |
| Credential rotation | §6.3 | M4 |
| Temp file security (umask) | §8.3 | M3 (when temp files are used) |

---

## 7. Next Step

After this spec is approved: invoke `superpowers:writing-plans` to produce a task-level implementation plan for the `nexus-security` crate.
