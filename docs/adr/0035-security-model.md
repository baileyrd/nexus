# ADR 0035: Security Model — Manifest Signing, At-Rest Encryption, TLS Pinning, and Audit Logging

**Date:** 2026-05-23
**Status:** Accepted (all mechanisms implemented as noted below)
**Related:** BL-099 (manifest signing), BL-101 (at-rest capability encryption), BL-102 (TLS pinning), `nexus-security::` crate, PRD-16

## Context

The Nexus shell is a Tauri application that executes third-party plugins (WASM + JavaScript) and communicates with external services (AI providers, LSP servers, DAP debuggers, MCP tool servers). The threat model covers:

- A malicious or compromised plugin reading/writing arbitrary files
- A compromised IPC channel between shell and kernel
- Man-in-the-middle attacks on AI provider connections
- Unauthorized access to stored credentials
- Plugin manifest tampering (claiming capabilities it does not have)
- Silent data exfiltration that leaves no trace

The `nexus-security` crate implements the security surface but no single ADR documented the overall model. BL-099, BL-101, and BL-102 were addressed in code and Cargo.toml comments but never captured in a decision record.

## Decision

**A four-pillar security model**, implemented in `nexus-security`:

### Pillar 1 — Manifest signing (BL-099)

- Every plugin manifest (JSON) is signed with **Ed25519-Dalek**
- On install, the Tauri shell verifies the signature against the known public key
- Unsigned manifests are rejected (hard-fail, not soft-fail)
- The signed manifest is stored alongside the plugin for runtime verification

### Pillar 2 — At-rest encryption of granted capabilities (BL-101)

- `granted_caps.json` (the file storing which capabilities a plugin has been granted by the user) is encrypted with **ChaCha20-Poly1305**
- The key is derived from the OS keyring credential (the same keyring that stores the plugin's signing public key)
- On plugin installation or capability grant, the file is re-encrypted
- On plugin uninstall, the granted caps are wiped

### Pillar 3 — TLS pinning (BL-102)

- All TLS connections use **`rustls` + `webpki-roots`** — no native-tls or openssl
- Root certificates are **pinned** to the `webpki-roots` trust store, not the OS store
- This prevents mitm attacks via compromised OS certificates
- `lettre` (SMTP with `rustls-tls`) also follows this model — no native-tls fallback
- `wasmtime` sandbox isolation with WASI capability is the plugin execution boundary

### Pillar 4 — Unified audit logging

- `nexus_kernel::audit` provides unified event logging for all security-relevant events
- Events include: plugin install/uninstall, capability grant/denial, TLS connection establishment, credential access, manifest verification
- The audit log lives under the app's config directory alongside shell-state.json
- No raw logging of secrets or user content — only metadata about security events

### `nexus_security::SecurityCorePlugin` exports

| Export | Purpose |
|---|---|
| `SecurityCorePlugin` | Core plugin registration |
| `CredentialVault` | OS keyring access via `nexus_security::credential` |
| `ForgePathValidator` | Safe path validation (preventing path traversal, symlink attacks) |
| `RiskLevel` enum + `risk_level()` | Risk classification for security operations |
| `tls::` module | TLS configuration (rustls, pinned roots) |
| `tls_pins::` module | Root certificate pinning |
| `ipc::` module | Inter-process security (IPC message signing, capability enforcement) |

## Consequences

### Positive

- **Defense in depth.** Four independent pillars each address a different attack vector. No single failure compromises the entire security model.
- **Explicit, not implicit.** Every mechanism is documented in `Cargo.toml comments` and implemented in `nexus-security::`. Future contributors can trace "why we do X this way" without digging through git history.
- **No openssl dependency.** All TLS via `rustls` — no native-tls fallback, no openssl on Android/iOS/etc. This also eliminates a significant class of supply-chain risk.
- **Audit trail exists.** Security events are logged. When a user asks "what did my plugins do today?", the audit log has the data.

### Negative / costs

- **Manifest signing adds install complexity.** Users (or installers) must have the signing key. The shell does not accept unsigned plugins — users cannot install community plugins from arbitrary sources without first obtaining the key.
- **At-rest encryption is selective.** Only `granted_caps.json` is encrypted, not shell-state.json or other config files. Other sensitive paths (credentials, auth tokens) are handled by the OS keyring, not by encryption.
- **TLS pinning to webpki-roots.** If Nexus needs to connect to a server with a self-signed or non-standard CA certificate, the default pinning prevents connection. A manual trust override mechanism needs to be designed.
- **No documented threat model for agent runtime sandboxing.** WASI confinement exists in `wasmtime` but no document covers the threat model for agent codebases executing in the shell's webview. A separate ADR is needed for this.

## Open follow-ups

1. **Document the threat model for agent runtime sandboxing.** ADR 0035 covers plugin/plugin-registry security but not the execution sandbox for AI agent codebases. This is explicitly flagged as a gap in the `nexus-security::` crate docs.
2. **Test all four pillars.** The audit noted `nexus-security` has no tests. Integration tests should cover: manifest signing verification, capability encryption/decryption round-trip, TLS pinning (successful connection with pinned cert, failed connection with unpinned cert), audit log entries for known security events.
3. **TLS relay.** The collaboration relay (ADR 0034) uses bare `ws://`. This is a known gap but requires its own ADR rather than piggybacking on ADR 0035, since the threat model for relay-vs-external-service connections differs from the TLS pinning model for provider connections.
