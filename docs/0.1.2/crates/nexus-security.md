# nexus-security

> Kind: lib · IPC plugin id: com.nexus.security · CorePlugin: yes · Has settings: no (own crate); reads `KernelConfig::tls_pinning_enabled` · As of: 2026-05-25

## Overview

`nexus-security` is the security subsystem of the microkernel. Its Cargo description — "Nexus security: capability risk metadata, credential vault, audit logging, path validation" — captures the four roles it plays, but only one of them is a live IPC surface. As a `CorePlugin` registered under `com.nexus.security`, it exposes a credential vault (over the OS keyring), read/clear access to the persisted audit log, and a kernel-metrics snapshot, all reachable uniformly from CLI/TUI/MCP/shell through `context.ipc_call("com.nexus.security", command, args)`. Its other three roles are plain library functions consumed in-process by other crates: the capability **risk-level table** (`risk_level`), the **forge-path validator** (re-exported from `nexus-types`), and the **TLS-pinning verifier/HTTP-client builder** (BL-102), which `nexus-ai` and `nexus-audio` call directly to get one shared pin policy for outbound HTTPS.

The crate sits deliberately *above* the kernel in layering but is registered **first** at bootstrap (`nexus-bootstrap/src/plugins/security.rs`) with `or_critical` — a lifecycle hang or failure aborts boot rather than running with capability gates or audit emission missing. The audit-emission helpers themselves (`audit::log_*`) physically live in `nexus-kernel` and are merely re-exported here (`pub use nexus_kernel::audit;`) so the kernel and the plugin host can emit audit events without a dependency cycle back through `nexus-security`. Likewise the audit *store* trait (`AuditStore`) lives in the kernel and its SQLite backend in `nexus-bootstrap`; this crate only *queries and clears* it through `nexus_kernel::audit_store`.

The vault enforces ADR-0009's hard-fail policy: `on_init` probes the OS keyring, and an unreachable keyring aborts plugin init with a platform-specific remediation hint. The `NEXUS_NO_KEYRING=1` escape hatch lets the system boot with a disabled vault — individual credential operations then fail with `KeyringDisabled` rather than blocking startup. Credentials live in the OS keyring (not in `.forge/`), so they are explicitly **not** part of the file-as-truth surface; the persisted audit log is derived/per-machine state at `<forge>/.forge/.kernel/audit.db`.

Note one architectural seam worth flagging up front: the kernel-tier capabilities `security.write` / `security.audit.write` are defined and risk-mapped here (both High), but the plugin's own `dispatch` does **not** check them — there is currently no per-handler capability gate on `set_secret` / `delete_secret` / `clear_audit_log`. See [Capabilities](#capabilities).

## Position in the dependency graph

- **Direct nexus-\* dependencies:**
  - `nexus-kernel` — for `EventBus` (lifecycle/audit event publish), `Capability` (risk table + `CapabilityDenied` error), and the `audit` / `audit_store` / `metrics` modules it re-exports and queries.
  - `nexus-plugins` — for the `CorePlugin` trait and `PluginError` (the IPC return error type).
  - `nexus-types` — for `ForgePathValidator` / `PathValidationError`, which actually live in that leaf crate (so kernel/plugins can call the validator without cycling through security); this crate re-exports the type and adds the `From<PathValidationError> for SecurityError` conversion.
- **Notable external dependencies (+ why):**
  - `keyring` — the OS keyring backend behind `CredentialVault` (Secret Service / Keychain / Credential Manager).
  - `rustls`, `rustls-pki-types`, `webpki-roots` — BL-102 TLS pinning: a custom `ServerCertVerifier` that delegates chain validation to `WebPkiServerVerifier` seeded with the Mozilla root store, then adds a leaf-cert hash check.
  - `sha2` — SHA-256 of the leaf certificate DER for pin comparison.
  - `reqwest` — `build_pinned_client` / `pinned_client_config` build the outbound HTTPS client; lives here so `nexus-ai` and `nexus-audio` share one pin policy.
  - `ts-rs` + `schemars` (optional, behind the `ts-export` feature) — emit TypeScript bindings + JSON Schema for the wire types in `ipc.rs`; off by default, enabled by `scripts/check_ipc_drift.sh`.
  - `tempfile` (dev) — temp forge roots for path-validator tests.
- **Crates that depend on this one:** `nexus-bootstrap` (registers the core plugin), `nexus-ai` and `nexus-audio` (TLS-pinned HTTP client), `nexus-git` and `nexus-collab` and `nexus-cli` (link it per `Cargo.toml`). No subsystem crate reaches the security *IPC handlers* directly — that goes through `ipc_call`.

## Public API surface

The crate root (`src/lib.rs`) declares `#![deny(missing_docs)]` and `#![warn(clippy::pedantic)]`, re-exports `nexus_kernel::audit`, and surfaces:

- **`core_plugin`** (public module) — `SecurityCorePlugin` (re-exported at root) plus the IPC handler-id constants and the `IPC_HANDLERS: &[(&str, u32)]` table that bootstrap consumes (SD-06 single source of truth). `SecurityCorePlugin::new(event_bus)` (production keyring probe), `SecurityCorePlugin::with_probe(event_bus, probe)` (test injection; uses a permanently-disabled vault), and `publish_audit(event_type, payload)` (best-effort publish under `com.nexus.security.audit.*`).
- **`credential`** (private module; `CredentialVault` re-exported at root) — `new()` (honours `NEXUS_NO_KEYRING`), `disabled()` (always disabled, no env read), `available()`, `store(name, value)`, `retrieve(name)`, `delete(name)`, `is_disabled()`. Implements `Default`.
- **`error`** (private; `SecurityError` re-exported) — the unified error enum: `KeyringUnavailable{reason, platform_hint}`, `KeyringDisabled`, `CredentialNotFound`, `CredentialStoreFailed`, `PathTraversal`, `InvalidPath`, `CapabilityDenied(Capability)`, `CertificatePinMismatch{host, expected, actual}`, `NoPinsConfigured{host}`.
- **`ipc`** (public module) — wire-mirror request/response structs (see [IPC handlers](#ipc-handlers)): `GetSecretArgs/Result`, `SetSecretArgs/Result`, `DeleteSecretArgs/Result`, `ListSecretNamesArgs/Result`, `QueryAuditLogArgs` + `AuditLogEntry`, `ClearAuditLogArgs/Result`. All `#[serde(deny_unknown_fields)]`; conditionally derive `TS` + `JsonSchema` under `ts-export`.
- **`path`** (private; `ForgePathValidator` re-exported) — re-export of `nexus_types::{ForgePathValidator, PathValidationError}` plus the `From<PathValidationError> for SecurityError` impl.
- **`risk`** (private; `risk_level` + `RiskLevel` re-exported) — `RiskLevel { Low, Medium, High }` (with `is_high()` and uppercase `Display`) and `fn risk_level(cap: Capability) -> RiskLevel`, an **exhaustive** match over `Capability` (adding a kernel capability breaks compilation here until mapped).
- **`tls`** (public module, BL-102) — `PinnedServerCertVerifier` (impl `ServerCertVerifier`), `PinnedServerCertVerifier::new_with_webpki_roots()`, `pinned_client_config() -> rustls::ClientConfig`, `build_pinned_client(tls_pinning_enabled: bool) -> reqwest::Client`.
- **`tls_pins`** (public module, BL-102) — `HOST_PINS: &[(&str, &[&str])]` (ships **empty**) and `pins_for_host(host) -> &'static [&'static str]` (case-insensitive, trailing-dot stripped).

## IPC handlers

Registered by `nexus-bootstrap` from `core_plugin::IPC_HANDLERS` (each also aliased with a `v1.` prefix via `with_v1_aliases`). Dispatched in `SecurityCorePlugin::dispatch` by numeric handler id. Args/returns are the JSON shapes the handler reads/writes (the `ipc::*` structs are the typed mirrors).

| Command | Handler id | Args | Returns | Capability required | Description |
|---------|-----------|------|---------|---------------------|-------------|
| `get_secret` | 1 | `{plugin_id, name}` | `{value: string\|null}` | none enforced | Reads the keyring entry keyed `"{plugin_id}:{name}"`. `CredentialNotFound` **and** `KeyringDisabled` are softened to `{value: null}` so callers can fall through to a default; other errors propagate as `ExecutionFailed`. |
| `set_secret` | 2 | `{plugin_id, name, value}` | `{ok: true}` | none enforced (intended `security.write`, High) | Stores under `"{plugin_id}:{name}"` and records the key in the in-memory `known_names` index. In disabled mode `store` returns `KeyringDisabled`, surfaced as `ExecutionFailed` (set is **not** softened — the caller must know the secret was not persisted). |
| `delete_secret` | 3 | `{plugin_id, name}` | `{ok: bool}` | none enforced (intended `security.write`, High) | Deletes `"{plugin_id}:{name}"`. Success and `CredentialNotFound` both return `{ok: true}` (and drop the name from the index); `KeyringDisabled` soft-fails to `{ok: false}`; other errors propagate. |
| `list_secret_names` | 4 | `{plugin_id}` | `{names: string[]}` | none enforced | Returns the short names in `known_names` whose key has the `"{plugin_id}:"` prefix. **Session-scoped**: the OS keyring cannot enumerate, so only names set during the current session appear; names from prior sessions remain retrievable by exact name but are not listable. Never returns values. |
| `query_audit_log` | 5 | `{event_type?, plugin_id?, since_ts?, limit?}` | `Vec<AuditLogEntry>` (`{id, ts_ms, event_type, plugin_id?, detail_json}`) | none enforced | Builds a `nexus_kernel::audit_store::AuditQuery` from the optional filters and returns rows newest-first (default limit 1000). Returns `[]` if no audit store is installed. (BL-094) |
| `clear_audit_log` | 6 | `{before_ts: i64}` | `{removed: u64}` | none enforced (intended `security.audit.write`, High) | Deletes audit rows with `ts_ms < before_ts` via `audit_store::clear`. Missing `before_ts` → `ExecutionFailed`. Backs `nexus logs clear --older-than`. (BL-100) |
| `metrics_snapshot` | 7 | ignored | `MetricsSnapshot` JSON (or `null` if no global metrics registry) | none enforced | Snapshots `nexus_kernel::metrics::global()`. Read-only observability. (BL-093) |

Unknown handler ids return `PluginError::ExecutionFailed`. Argument extraction uses `string_arg` (required string fields → `ExecutionFailed` if missing) and `vault_key` (builds the `"{plugin_id}:{name}"` namespaced key).

## Capabilities

This crate is the **owner of the capability risk model**, not (today) an enforcer of it on its own IPC surface.

- **Risk table (`risk::risk_level`)** — the canonical mapping from every `Capability` variant to `RiskLevel { Low, Medium, High }`. Used at community-plugin install time: High-risk capabilities require explicit user approval. The match is exhaustive, so a new kernel capability is a compile error until classified. The capability-inventory test (`tests/capability_inventory_emit.rs`) renders this table to `docs/generated/capabilities.md`, and `scripts/check_ipc_drift.sh` `git diff`s it so an unrun generator fails CI. High-band examples: `fs.read.external`, `fs.write.external`, `net.http`, `process.spawn`, `ipc.call`, `ai.config.write`, `audio.record`, `protocol-host.contribute`, `network.bind`, and `security.write` / `security.audit.write`.
- **`security.write` / `security.audit.write`** — defined in `nexus-plugin-api` (`Capability::SecurityWrite` → `"security.write"`, `Capability::SecurityAuditWrite` → `"security.audit.write"`) and mapped High here, with a comment stating they "gate" keyring writes and `clear_audit_log`. **Gap (accurate as of 2026-05-25):** a workspace grep finds these variants referenced only in `capability.rs` (definition + string mapping) and `risk.rs` (risk mapping). No `require_capability(Capability::SecurityWrite)` (or `SecurityAuditWrite`) call exists anywhere, and `SecurityCorePlugin::dispatch` performs no capability check. `docs/0.1.2/ipc-handlers.md` corroborates this, listing the security handlers' capability column as `—` and tagging the write/clear handlers as "candidates" for those caps. So the intent is documented and the caps exist, but the gate is not yet wired into the dispatch path.
- **TLS pinning specifics (BL-102):** not a `Capability` — it is a transport policy, gated by the boolean `KernelConfig::tls_pinning_enabled` (and the `NEXUS_TLS_PINNING=1` env opt-in). When on, every outbound HTTPS handshake to a host with configured pins must present a leaf certificate whose SHA-256 matches a pin; pinning is *in addition to* CA chain validation, never a replacement. An empty pin list for a target host under an enabled flag **fails closed** (`rustls::Error::General`), so opting in cannot silently weaken security.

`SecurityError::CapabilityDenied(Capability)` exists for callers that *do* enforce caps elsewhere (its `Display` shows the capability string, e.g. `net.http`), but it is not produced by this crate's own handlers.

## Settings / Config

The crate defines **no** `Config` struct and owns **no** `.forge/` TOML file — nothing in `docs/0.1.2/settings/` belongs to it. Its behaviour is driven by env vars and one externally-owned config field:

- **`NEXUS_NO_KEYRING`** — if exactly `"1"`, `CredentialVault::new()` returns a disabled vault: `available()` is `Ok(())` (boot proceeds) but `store`/`retrieve`/`delete` return `KeyringDisabled` (ADR-0009 escape hatch).
- **`NEXUS_TLS_PINNING`** — if `"1"`, forces `build_pinned_client` to enable pinning regardless of the passed flag.
- **`KernelConfig::tls_pinning_enabled`** — owned by `nexus-kernel` (`config.rs`), **defaults to `false`**. Passed into `build_pinned_client(tls_pinning_enabled)` by callers (`nexus-ai`, `nexus-audio`).
- **Pin table** — `tls_pins::HOST_PINS` is a compile-time constant, **shipped empty**. Seeding requires editing the source and rebuilding (the module documents the `openssl s_client` capture procedure; recommends ≥2 pins per host for rotation).

Persistence locations:

- **Credentials** — the OS keyring, service name `"nexus"`, entry name `"{plugin_id}:{name}"`. Not in `.forge/`, not file-as-truth.
- **Audit log** — `<forge>/.forge/.kernel/audit.db` (SQLite). The store is the `SqliteAuditStore` implemented in `nexus-bootstrap/src/audit_sqlite.rs` and installed into the kernel's global slot at boot; this crate only queries/clears it. On open it prunes rows older than `nexus_types::constants::AUDIT_LOG_RETENTION_DAYS`. Derived/per-machine state.

## Events

- **Published** (via `EventBus::publish_plugin`, all under the `com.nexus.security` source):
  - `com.nexus.security.started` — on `on_start`.
  - `com.nexus.security.stopped` — on `on_stop`.
  - `com.nexus.security.audit.<event_type>` — emitted by `publish_audit(...)` (best-effort; a failed publish logs `tracing::error!` and drops the event). This is a separate channel from the persisted audit log: the `audit::log_*` helpers write to the SQLite audit store + `tracing`, whereas `publish_audit` puts events on the live bus for subscribers (TUI, other plugins).
- **Subscribed:** none. The plugin holds an optional `Arc<EventBus>` only to publish.

Lifecycle (`CorePlugin`): `on_init` runs the keyring probe (ADR-0009 hard-fail → `PluginError::LifecycleError` carrying the platform hint); `on_start` / `on_stop` publish the lifecycle events above.

## Internals & notable implementation details

**Credential vault & at-rest encryption.** `CredentialVault` is a thin wrapper over `keyring-rs`; it stores nothing itself and provides *no* additional at-rest encryption — confidentiality is whatever the OS keyring provides (Secret Service/gnome-keyring/KWallet on Linux, Keychain on macOS, Credential Manager on Windows). The service name is the constant `"nexus"`; entries are keyed by the namespaced `"{plugin_id}:{name}"` so one plugin cannot read another's secrets through the IPC surface. `available()` probes by `get_password` on a `"__nexus_probe__"` entry and treats `keyring::Error::NoEntry` as success (keyring works, entry merely absent). `platform_error` maps any other keyring failure to `KeyringUnavailable` with a `cfg!(target_os = …)`-selected remediation hint. Every `store`/`retrieve`/`delete` calls `audit::log_credential_access(name, action)` first — the credential **value is never logged**.

**Session-scoped name index.** Because the OS keyring offers no enumeration, `SecurityCorePlugin` keeps an in-memory `HashSet<String>` (`known_names`) of namespaced keys set this session. `set_secret` inserts, `delete_secret` removes, and `list_secret_names` filters by the `"{plugin_id}:"` prefix. Cleared on plugin restart — a documented limitation reflected in `ListSecretNamesResult`'s doc comment.

**Audit log schema.** Owned by `nexus-bootstrap`'s `SqliteAuditStore`: table `audit_events(id INTEGER PK AUTOINCREMENT, ts_ms INTEGER, event_type TEXT, plugin_id TEXT NULL, detail_json TEXT)` with indices on `ts_ms`, `event_type`, `plugin_id`. `detail_json` is an opaque JSON string the caller parses. Queries run `WHERE (?1 IS NULL OR event_type = ?1) AND …` newest-first with a default limit of 1000; appends and clears are best-effort (errors logged + swallowed so audit never breaks the recorded operation). Event types come from the kernel's `audit::log_*` helpers: `capability_granted`, `capability_revoked`, `capability_denied`, `plugin_lifecycle`, `credential_access`, `mcp_tool_call`, `mcp_resource_read`, `path_traversal_denied`.

**TLS certificate pinning (BL-102).** `PinnedServerCertVerifier` wraps `rustls::client::WebPkiServerVerifier` (Mozilla roots via `webpki-roots`). On `verify_server_cert` it first runs standard chain validation, then resolves the DNS host name, looks up `tls_pins::pins_for_host`, and compares. Notable design choices documented in the source:
- **Hash domain.** Despite the module/`tls_pins` docs and `error.rs` referring to **SPKI** SHA-256, the shipped `leaf_fingerprint` actually hashes the **full leaf certificate DER** (`SHA-256(cert.as_ref())`) to avoid ASN.1 parsing. Switching to true SPKI pinning is a documented one-line follow-up in `leaf_fingerprint` (the lowercase-hex on-disk format is identical either way). This is a doc/impl mismatch worth knowing.
- **Fail-closed empties.** With pinning on, an empty pin list for the host returns `rustls::Error::General` rather than allowing the connection.
- **Non-DNS server names** (IP literals) are rejected with `Error::General`.
- **Crypto provider.** `pinned_client_config` best-effort-installs `rustls::crypto::ring` as the process default (idempotent) to avoid a "CryptoProvider not selected" panic across mixed rustls feature graphs.
- **Graceful degradation.** `build_pinned_client` returns a stock `reqwest::Client::new()` when pinning is off; when on but client construction fails, it logs `tracing::warn!` and falls back to a stock client so a misconfigured pin pipeline never leaves a caller without an HTTP client. Shared by `nexus-ai` (also re-exported via `nexus-ai/src/http_client.rs`) and `nexus-audio`.

Note the `SecurityError::CertificatePinMismatch` / `NoPinsConfigured` variants are defined for a future security-error-surfaced path, but the *running* verifier returns `rustls::Error::General` strings (the rustls trait can only return `rustls::Error`); the typed `SecurityError` pin variants are not produced by `tls.rs` today.

**Path validation.** `ForgePathValidator` itself lives in `nexus-types` (leaf crate) so kernel/plugins can call it on write paths without cycling through security. This crate only re-exports it and provides `From<PathValidationError> for SecurityError` (`PathTraversal` / `InvalidPath`) so security-side call sites get the unified error surface.

## Tests

- **`tests/prd-02-smoke.rs`** (PRD-02 §12 public-surface smoke): `risk_level` covers all `Capability::ALL` and the five High caps; `CredentialVault::disabled()` mode (`available` Ok, store/retrieve/delete all `KeyringDisabled`); `ForgePathValidator` allows a real file and blocks `../../../etc/passwd`; all `audit::log_*` helpers are callable without panicking; `SecurityError` Display readability; and that every public type is importable/constructible. Explicitly does **not** touch the real OS keyring.
- **`tests/capability_inventory_emit.rs`** (BL-137): regenerates `docs/generated/capabilities.md` from `Capability::ALL` × `risk_level` and asserts the write/read roundtrip; real drift is caught by `scripts/check_ipc_drift.sh`.
- **Inline `#[cfg(test)]` modules:**
  - `core_plugin.rs` — plugin id; `on_init` ok/fail/disabled paths via injected probe (asserts `LifecycleError` carries the D-Bus + gnome-keyring hint); start/stop without a bus; unknown-handler error; `get_secret` → null and `set_secret` → error in disabled mode; `set_secret` missing `plugin_id`; `list_secret_names` prefix filtering; `delete_secret` → `{ok:false}` disabled; and that `on_start` publishes `com.nexus.security.started` to a subscribed bus.
  - `credential.rs` — disabled-vault behaviour, `disabled()` bypasses env, `new()` not disabled when `NEXUS_NO_KEYRING != "1"`, and `platform_error` carries a non-empty hint. (No test exercises a live keyring — by design.)
  - `error.rs` — Display content for every error variant.
  - `path.rs` — `PathValidationError` → `SecurityError` conversion for both variants.
  - `risk.rs` — exhaustive coverage assertion plus per-capability spot checks, `is_high`, and uppercase `Display`.
  - `tls.rs` — empty-input SHA-256 vector check and that `pinned_client_config` builds without panic.
  - `tls_pins.rs` — unknown host → empty pins; case-insensitive + trailing-dot normalisation (positive-match test deferred until real pins land, since `HOST_PINS` ships empty).

No integration test exercises the IPC handlers end-to-end through the kernel dispatcher, and there is no test asserting capability gating on `set_secret`/`clear_audit_log` (consistent with the un-wired gate noted above).
