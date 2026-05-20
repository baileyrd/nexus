# com.nexus.security

- **Path:** `crates/nexus-security/`
- **Tier:** Core Rust
- **Bootstrap order:** 1 (registered first so audit events route through it before other plugins emit)

## Architecture

- Entry point: `crates/nexus-security/src/core_plugin.rs` (`SecurityCorePlugin`). Re-exported by `crates/nexus-security/src/lib.rs`.
- Bootstrap wiring: `crates/nexus-bootstrap/src/plugins/security.rs:14`. Manifest is built inline from `IPC_HANDLERS` via `core_manifest_with_ipc`; no `plugin.toml` file on disk.
- Key modules:
  - `credential.rs` — `CredentialVault` wrapping the OS keyring (`keyring` crate). Supports `NEXUS_NO_KEYRING=1` "disabled" mode per ADR-0009.
  - `risk.rs` — capability risk metadata (`RiskLevel`).
  - `path.rs` — `ForgePathValidator` (forge-relative path safety).
  - `tls.rs`, `tls_pins.rs` — BL-102 TLS pinning verifier and per-host pin table; shared `reqwest` client builder used by `nexus-ai` and `nexus-audio`.
  - `ipc.rs` — wire types for the IPC handlers.
- Lifecycle: `on_init` probes the keyring (hard-fail per ADR-0009 unless `NEXUS_NO_KEYRING=1`); `on_start` / `on_stop` publish `com.nexus.security.started` / `.stopped` on the kernel bus.
- Audit events flow through `nexus_kernel::audit_store` — re-exported from `nexus-kernel` to avoid a dep cycle (`lib.rs:14`).
- Persistence: none of its own. The kernel-side `audit_store` writes `<forge>/.forge/.kernel/audit.db` (created by `nexus-bootstrap::audit_sqlite`); this plugin only reads/prunes it via handlers 5/6.
- Settings owned: none directly. The `NEXUS_NO_KEYRING` and `NEXUS_TLS_PINNING` env vars are documented in `docs/0.1.2/settings/env-vars.md`.
- External dependencies of note: `keyring` (OS keychain — D-Bus on Linux, Keychain on macOS, Credential Manager on Windows), `rustls` + `rustls-pki-types` + `webpki-roots`, `reqwest`.

## Surface

IPC commands (from `core_plugin.rs:47` `IPC_HANDLERS`):

| Id | Command             | Purpose                                                 |
|---:|---------------------|---------------------------------------------------------|
|  1 | `get_secret`        | Retrieve a secret by `(plugin_id, name)`                |
|  2 | `set_secret`        | Store a secret                                          |
|  3 | `delete_secret`     | Remove a secret                                         |
|  4 | `list_secret_names` | List in-session names for `plugin_id`                   |
|  5 | `query_audit_log`   | Query persisted audit log (BL-094)                      |
|  6 | `clear_audit_log`   | Prune audit entries older than `before_ts` (BL-100)     |
|  7 | `metrics_snapshot`  | Snapshot the kernel-metrics registry (BL-093)           |

Events: `com.nexus.security.started`, `com.nexus.security.stopped`, `com.nexus.security.audit.*` (best-effort via `publish_audit`).

## Necessity

- **Verdict:** Essential.
- **Required for basic capabilities?** Yes. The plugin runs first in bootstrap and gates further init via the ADR-0009 hard-fail keyring probe. Without it (or its disabled-mode escape hatch), the rest of the registration sequence aborts. It also owns the shared TLS-pinning HTTP client used by `nexus-ai` / `nexus-audio` and the audit-log read API the rest of the system surfaces.
- **Depended on by:** indirectly the entire boot sequence (registered before `storage` so audit emissions during storage init have a route); `nexus-ai` and `nexus-audio` link the crate for `tls::*`.
- **Depends on:** `nexus-kernel` (event bus, audit store, metrics), `nexus-plugins`, `nexus-types`. No service-plugin deps.
- **What breaks if removed:** no plugin secrets, no audit-log query/prune IPC, no kernel-metrics snapshot, no shared TLS-pin policy for outbound HTTPS. Boot can technically proceed without the hard-fail probe, but downstream callers that already call `com.nexus.security::get_secret` (AI provider credentials, MCP auth) lose their credential path.

## Notes

- The plugin's manifest is constructed at bootstrap from the `IPC_HANDLERS` slice — there is no `plugin.toml` file on disk. ADR-0021 v1 aliases are auto-applied via `with_v1_aliases`.
- `list_secret_names` only returns names set in the current session (the OS keyring doesn't enumerate); prior-session secrets are still retrievable by exact name.
- Disabled-mode (`NEXUS_NO_KEYRING=1`) intentionally lets `on_init` succeed; individual credential ops then fail loudly with `KeyringDisabled`.
- `clear_audit_log` requires `before_ts`; missing arg surfaces as `ExecutionFailed` rather than a silent no-op.
