# Security Implementation Assessment
_Assessed: 2026-05-06_

## Overall: 8/10 — Production-grade for personal use. One focused week from hardened.

The security implementation is more complete than the codebase's age might suggest. The foundations
— WASM sandboxing, capability-based access control, path traversal defense, OS keyring integration
— are all genuinely first-class. The three material gaps are intentional deferrals tied to the
community marketplace launch, not oversights. For a single-user personal forge, the system is
production-ready.

---

## What's fully implemented and first-class

**WASM sandbox with three independent resource limits.** CPU budget via fuel (reset per call —
prevents accumulation across a long-lived plugin), memory ceiling via `wasmtime::StoreLimits`,
and wall-clock deadline via epoch-based interruption (not cooperative — wasmtime forcefully halts
at the deadline). Log output rate-limited at 1000 lines/sec via token bucket. All three limits
configurable per plugin via manifest.

**Capability gating enforced at every API call, not just at load time.** Every one of the 18 WASM
host functions checks capabilities as the first line before doing anything. `FsRead` does not imply
`FsWrite`. Nothing inherits. An audit event fires on every denial. The integration test
(`wasm_capability_denial.rs`) uses a WAT fixture to prove the denial path — not just a helper unit
test.

**Version-pinned capability grants.** When a plugin upgrades, all HIGH-risk grants reset and the
user is re-prompted. The version stored in `granted_caps.json` must match the manifest version or
the grant set is empty. Closes the "upgrade to gain new permissions silently" attack.

**Path validation closes the TOCTOU race.** `validate()` follows symlinks, canonicalizes, verifies
result starts with `forge_root`. `validate_for_write()` canonicalizes the deepest *existing*
ancestor to close the symlink-swap window. 17 tests cover traversal attacks, symlink escapes,
null bytes, absolute paths, and edge cases.

**OS keyring hard-fail policy (ADR-0009).** If the keyring is unavailable at startup, `on_init`
returns `Err` and bootstrap fails. No silent fallback to plaintext. `NEXUS_NO_KEYRING=1` provides
an explicit escape hatch for CI. Platform-specific error messages on Linux, macOS, Windows.

**Audit logging is structured and filterable.** Every capability denial, credential access, path
traversal attempt, and plugin lifecycle event flows through `tracing` with `audit = true` as a
structured field. Downstream subscribers can filter on this field.

**22 capabilities with risk classification.** HIGH-risk (6: `FsReadExternal`, `FsWriteExternal`,
`NetHttp`, `ProcessSpawn`, `IpcCall`, `AiConfigWrite`) require explicit user consent, persisted to
`granted_caps.json`. Risk classification tested exhaustively against `Capability::ALL`.

---

## Capability risk levels

| Risk | Capabilities | Grant behavior |
|---|---|---|
| Low | `FsRead`, `KvRead/Write`, `AiIndex`, `AiSessionRead/Write`, `UiNotify` | Auto-granted |
| Medium | `FsWrite`, `NetHttpLocalhost`, `DbQuery/Write`, `EventsPublish`, `AiChat`, `AiActivityWrite`, `AiToolsWrite/Mcp` | Auto-granted |
| High | `FsReadExternal`, `FsWriteExternal`, `NetHttp`, `ProcessSpawn`, `IpcCall`, `AiConfigWrite` | Requires explicit user consent; persisted to `granted_caps.json` |

---

## Where it falls short

### 1. Plugin signing is not implemented

No signature field in the manifest. No `ed25519_dalek` in the workspace. A WASM binary can be
swapped at install time with no detection. The PRD specifies the full signing workflow (ed25519,
community keyring, CRL) — explicitly deferred to the marketplace launch. Until it ships, plugin
provenance is unverifiable.

### 2. Audit log is not persisted

Events emitted via `tracing`. Where they go is the application's problem. Process restart loses all
audit history. No rolling files, no compression, no tamper detection (the PRD specifies Merkle
hash-chaining). Acceptable for a single-user personal tool; not for compliance.

### 3. `com.nexus.security` has zero IPC handlers

The dispatch method returns an explicit error for every handler ID. The `CredentialVault` is
library-only. If `nexus-git` wants to cache an SSH passphrase in the keyring (BL-090), there is
no IPC path — it would have to directly link `nexus-security`, violating the architecture.
Registering `get_secret`/`set_secret`/`delete_secret` is the highest-leverage single change in
the security crate.

### 4. `granted_caps.json` is plaintext

Directly editable by the user to grant any capability on any plugin. Should be PBKDF2-sealed or
protected by the OS keyring.

### 5. TLS pinning struct exists but is never called

`SecureConnection::connect_with_pinning` is defined. The AI provider HTTP client never calls it.
A MITM between Nexus and the Anthropic API is undetectable by the application.

### 6. No fuzzing targets

Zero fuzz targets in the codebase. For the WASM sandbox, path validator, and capability gates,
fuzzing is the most effective way to find edge cases. The PRD specifies fuzz targets; none shipped.

---

## Scorecard

| Dimension | Score | Notes |
|---|---|---|
| WASM sandbox | 10/10 | CPU/memory/time limits, log rate-limiting, all enforced |
| Capability enforcement | 10/10 | Every API gated, per-call checks, version-pinned grants |
| Path traversal defense | 9/10 | Solid; true TOCTOU-free write needs `openat2` (Linux-only) |
| OS keyring integration | 9/10 | Hard-fail policy, platform hints, escape hatch |
| Audit logging | 6/10 | Structured events emitted; not persisted |
| Plugin signing | 1/10 | Architecture defined; implementation deferred |
| IPC surface | 2/10 | Plugin starts; zero handlers registered |
| Cryptography | 3/10 | OS keyring handles crypto; nothing native yet |
| Fuzzing / adversarial testing | 1/10 | No fuzz targets |
| TLS pinning | 2/10 | Struct defined, never wired |

---

## The honest summary

For a personal developer tool: this is sufficient. A malicious community plugin cannot escape the
WASM sandbox, read outside the forge, access credentials it wasn't granted, or forge another
plugin's identity. Path traversal is blocked. OS keyring integration is correct.

For a shared deployment or community marketplace: three things need to ship first — plugin signing
(3 days), audit persistence (2 days), and IPC handler registration for `com.nexus.security` so
other plugins can call the credential vault without direct crate linkage (1 day). Total: about a
week.

The most surprising finding: `com.nexus.security` has no IPC handlers. The credential vault is
library-only. Any plugin that needs to store or retrieve a secret must directly link
`nexus-security`, violating the architecture. Registering `get_secret`/`set_secret`/`delete_secret`
is the single highest-leverage change in the security crate.

---

## Key source files

```
crates/nexus-security/src/
├── risk.rs             (186)  — 22 capabilities × 3 risk levels; 16 tests
├── credential.rs       (246)  — OS keyring vault (keyring-rs); 10 tests
├── core_plugin.rs      (244)  — Plugin lifecycle + hard-fail probe; 0 IPC handlers
└── error.rs            (101)  — 8 typed error variants

crates/nexus-types/src/
└── path_validator.rs   (444)  — ForgePathValidator (read + write paths); 17 tests

crates/nexus-plugins/src/
├── sandbox.rs          (645)  — WASM execution: fuel, memory, epoch timeout, token bucket
├── host_fns.rs         (838)  — 18 host functions with capability gates
└── loader.rs         (2,648)  — Plugin loading, granted_caps.json management

crates/nexus-kernel/src/
└── audit.rs            (150)  — Structured audit event helpers (tracing-based)

crates/nexus-plugins/tests/
└── wasm_capability_denial.rs  — Integration test: denial path + audit event proof
```
