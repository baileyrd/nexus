# AUDIT-flagged Handlers

> Every `# AUDIT:` row in `crates/nexus-bootstrap/cap_matrix.toml`. These handlers preserve pre-BL-138 behaviour (any caller with `ipc.call` can dispatch) but the security author has flagged them as candidates for cap elevation. Each is a deliberate decision — promoting them is non-trivial because it requires deciding what cap to require and updating every existing caller's capability grant.

## Severity table

| Handler | Concern | Suggested cap | Tracking |
|---------|---------|---------------|----------|
| `com.nexus.workflow::run` | Drives arbitrary plugins via `ipc_call` — each step is gated by the target handler's caps but the *aggregation* is not | per-step aggregation rule | issue #77 |
| `com.nexus.workflow::run_digest` | Same shape as `run`, cron-driven via the digest pipeline | per-step aggregation rule | issue #77 |
| `com.nexus.mcp.host::call_tool` | Invokes a tool on a previously-connected MCP server; side effects happen in the MCP server's own process. `connect` is `process.spawn`-gated, which restricts who could attach a server in the first place — this row preserves that posture but the host-side cap surface is still worth a second look | new `mcp.tool.call` or stack on `connect` | — |

## Historical promotions (`# AUDIT:` removed in current matrix)

| Handler | Now gated by | Removed in |
|---------|--------------|------------|
| `com.nexus.security::set_secret` | `caps = ["security.write"]` | P1-01 |
| `com.nexus.security::delete_secret` | `caps = ["security.write"]` | P1-01 |
| `com.nexus.security::clear_audit_log` | `caps = ["security.audit.write"]` | P1-01 |
| `com.nexus.ai::resolve_credentials` | `internal = true` (Core-trust callers only) | P1-02 |
| `com.nexus.terminal::send_input` | `caps = ["process.spawn"]` | P1-03 |
| `com.nexus.terminal::send_raw_input` | `caps = ["process.spawn"]` | P1-03 |
| `com.nexus.terminal::run_saved` | `caps = ["process.spawn"]` | P1-03 |
| `com.nexus.terminal::adhoc_promote` | `caps = ["process.spawn"]` | P1-03 |
| `com.nexus.terminal::repl_eval` | `caps = ["process.spawn"]` | P1-03 (BL-142) |
| `com.nexus.git::push` | `caps = ["net.http"]` | P1-04 |
| `com.nexus.git::push_tags` | `caps = ["net.http"]` | P1-04 |
| `com.nexus.linkpreview::fetch` | `caps = ["net.http"]` | P1-05 |
| `com.nexus.agent::delegate` | `caps = ["ai.chat"]` | P1-06 |
| `com.nexus.agent::plan` | `caps = ["ai.chat"]` | P1-06 |
| `com.nexus.collab::start_relay` | `caps = ["network.bind"]` | P1-07 (BL-143 Phase 2.3) |

This table is informational — the canonical state is the absence of a `# AUDIT:` comment on the row in `cap_matrix.toml`.

## Tracking issues

- **Issue #77** — workflow laundering (`run` / `run_digest` cap surface)
- **BL-099** — manifest signing (gating + revocation)
- **BL-101** — granted_caps.json at-rest encryption (shipped)
- **BL-102** — TLS pinning for AI providers (shipped, opt-in)
- **BL-138** — per-handler capability matrix (this file is its output)

## How to promote one of these

1. Decide the cap. If it's an existing cap (e.g. `net.http`), update the row's `unrestricted = …` to `caps = ["<cap>"]` and remove the `# AUDIT:` comment block. If it's a new cap (e.g. `security.write`), add it first to `crates/nexus-kernel/src/capability.rs::Capability::ALL` and to `crates/nexus-security/src/risk.rs::risk_level`.
2. Run `cargo test -p nexus-security --test capability_inventory_emit` to regenerate `docs/generated/capabilities.md`.
3. Run `cargo test -p nexus-bootstrap --test cap_matrix_complete -- --ignored` to confirm the matrix row covers every handler.
4. Walk every existing caller's `manifest.capabilities.required` and grant the cap where appropriate.
5. The drift check (`scripts/check_ipc_drift.sh`) will fail CI if generated files are stale.
6. Move the row from "Severity table" to "Historical promotions" in this doc.

## Keeping this doc in sync

To regenerate the live row count and spot any drift from cap_matrix:

```bash
grep -n "# AUDIT" crates/nexus-bootstrap/cap_matrix.toml
```

Expect three hits at v0.1.2 (the three rows in the Severity table above). A future drift script should fail CI if the doc lists handlers no longer flagged.
