# AUDIT-flagged Handlers

> **As of:** 2026-05-22. Every still-unrestricted handler in `crates/nexus-bootstrap/cap_matrix.toml` that the security author has flagged as a candidate for cap elevation. Each is a deliberate decision — promoting one is non-trivial because it requires deciding what cap to require and updating every existing caller's capability grant.
>
> A regression guard (`scripts/check_ipc_docs_drift.sh`) compares this table to the matrix on every CI run and fails the build if either side drifts.

## Severity table

| Handler | Concern | Suggested cap |
|---------|---------|---------------|
| `com.nexus.ai::resolve_credentials` | Returns provider keyring material via the agent/audio call paths. Currently restricted at the trust layer via the matrix's `internal = true` marker (Core-trust callers only); promoting it to an explicit cap is still tracked. | in-tree-only marker (live) → future explicit cap |
| `com.nexus.mcp.host::call_tool` | Invokes a tool on a previously-connected MCP server whose side effects (file writes, network calls) happen in the MCP server's own process. The `connect` spawn gate already restricts who could attach a server in the first place, so this preserves that posture. | track under MCP scoping work |
| `com.nexus.workflow::run` | Drives arbitrary plugins via `ipc_call` — issue #77 "laundering surface". Each step it dispatches is still gated by the target handler's caps, but the *aggregation* of side effects across a multi-step workflow is not capped at the workflow boundary. | track per-step aggregation rule (issue #77, BL-134 Phase 3) |
| `com.nexus.workflow::run_digest` | Same shape as `run`, cron-driven via the digest scheduler. | same as `run` |

## Closed since the 2026-05-21 audit

The following handlers were classified as `unrestricted` on 2026-05-21 and are now properly cap-gated in the matrix. No change to the suggested caps was needed — the migration was a `unrestricted = "…"` → `caps = […]` swap on each row:

| Handler | Gate | When |
|---------|------|------|
| `com.nexus.security::set_secret` | `security.write` | pre-2026-05-22 |
| `com.nexus.security::delete_secret` | `security.write` | pre-2026-05-22 |
| `com.nexus.security::clear_audit_log` | `security.audit.write` | pre-2026-05-22 |
| `com.nexus.terminal::send_input` | `process.spawn` | pre-2026-05-22 |
| `com.nexus.terminal::send_raw_input` | `process.spawn` | pre-2026-05-22 |
| `com.nexus.terminal::run_saved` | `process.spawn` | pre-2026-05-22 |
| `com.nexus.terminal::adhoc_promote` | `process.spawn` | pre-2026-05-22 |
| `com.nexus.terminal::repl_eval` | `process.spawn` | pre-2026-05-22 |
| `com.nexus.git::push` | `net.http` | pre-2026-05-22 |
| `com.nexus.git::push_tags` | `net.http` | pre-2026-05-22 |
| `com.nexus.linkpreview::fetch` | `net.http` | pre-2026-05-22 |
| `com.nexus.agent::delegate` | `ai.chat` | pre-2026-05-22 |
| `com.nexus.agent::plan` | `ai.chat` | pre-2026-05-22 |
| `com.nexus.collab::start_relay` | `network.bind` | pre-2026-05-22 |

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
