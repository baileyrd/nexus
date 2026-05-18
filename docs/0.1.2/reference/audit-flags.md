# AUDIT-flagged Handlers

> Every `# AUDIT:` row in `crates/nexus-bootstrap/cap_matrix.toml`. These handlers preserve pre-BL-138 behaviour (any caller with `ipc.call` can dispatch) but the security author has flagged them as candidates for cap elevation. Each is a deliberate decision — promoting them is non-trivial because it requires deciding what cap to require and updating every existing caller's capability grant.

## Severity table

| Handler | Concern | Suggested cap |
|---------|---------|---------------|
| `com.nexus.security::set_secret` | A hostile plugin with `ipc.call` could overwrite stored secrets | new `security.write` (or in-tree-only marker) |
| `com.nexus.security::delete_secret` | Same — could erase stored secrets | new `security.write` |
| `com.nexus.security::clear_audit_log` | Destroys audit history — the surface a hostile caller would target to cover its tracks | new `security.audit.write` |
| `com.nexus.ai::resolve_credentials` | Returns provider keyring material via the agent/audio call paths | in-tree-only marker |
| `com.nexus.terminal::send_input` | Writes commands into a live PTY (the shell was `process.spawn`-gated but this isn't) | `process.spawn` |
| `com.nexus.terminal::send_raw_input` | Same byte-level | `process.spawn` |
| `com.nexus.terminal::run_saved` | Replays a saved command into a live PTY | `process.spawn` |
| `com.nexus.terminal::adhoc_promote` | Promotes ad-hoc → saved and runs it | `process.spawn` |
| `com.nexus.terminal::repl_eval` | Writes code into a REPL kernel PTY (BL-142) | `process.spawn` |
| `com.nexus.git::push` | Outbound network reach to a remote | `net.http` |
| `com.nexus.git::push_tags` | Same — publishes tags | `net.http` |
| `com.nexus.linkpreview::fetch` | Outbound HTTP to arbitrary URLs | `net.http` |
| `com.nexus.workflow::run` | Drives arbitrary plugins via ipc_call — issue #77 "laundering surface" | track per-step gating |
| `com.nexus.workflow::run_digest` | Same shape, cron-driven | track per-step gating |
| `com.nexus.agent::delegate` | Drives a chat call internally (same machinery as session_run, which requires `ai.chat`) | `ai.chat` |
| `com.nexus.agent::plan` | Drives a chat call internally | `ai.chat` |
| `com.nexus.collab::start_relay` | Binds 0.0.0.0 (Share-this-forge surface) | new `network.bind` |

## Tracking issues

- **Issue #77** — workflow laundering (run / run_digest cap surface)
- **BL-099** — manifest signing (gating + revocation)
- **BL-101** — granted_caps.json at-rest encryption (shipped)
- **BL-102** — TLS pinning for AI providers (shipped, opt-in)
- **BL-138** — per-handler capability matrix (this file is its output)

## How to promote one of these

1. Decide the cap. If it's an existing cap (e.g. `net.http`), update the row's `unrestricted = …` to `caps = ["net.http"]`. If it's a new cap (e.g. `security.write`), add it first to `crates/nexus-kernel/src/capability.rs::Capability::ALL` and to `crates/nexus-security/src/risk.rs::risk_level`.
2. Run `cargo test -p nexus-security --test capability_inventory_emit` to regenerate `docs/generated/capabilities.md`.
3. Run `cargo test -p nexus-bootstrap --test cap_matrix_complete -- --ignored` to confirm the matrix row covers every handler.
4. Walk every existing caller's `manifest.capabilities.required` and grant the cap where appropriate.
5. The drift check (`scripts/check_ipc_drift.sh`) will fail CI if generated files are stale.
