# com.nexus.acp

- **Path:** `crates/nexus-acp/`
- **Tier:** Core Rust
- **Bootstrap order:** 21

## Architecture

- Two complementary roles in one crate. **Host** (BL-144 / ADR 0027 Phase 4, outbound) is the `AcpCorePlugin` — spawns ACP-speaking agent sub-processes contributed by community plugins, proxies request/response, republishes notifications on the bus. **Server** (BL-145 / Hermes Feature 7, inbound) is `AcpServer` — a line-delimited JSON-RPC 2.0 stdio surface that exposes a subset of `com.nexus.agent` verbs to external Hermes-compatible clients; started by the `nexus acp serve` CLI binary.
- Wire framing is newline-delimited JSON (matches Hermes Feature 7), not LSP's `Content-Length:` header framing. See `crates/nexus-acp/src/lib.rs` module docs.
- Entry point `crates/nexus-acp/src/lib.rs` re-exports `AcpCorePlugin`, `AcpHostConfig`, `AcpServer`, `ConnectionPool`. Registered by `crates/nexus-bootstrap/src/plugins/acp.rs` with all three lifecycle hooks enabled.
- Key modules: `core_plugin`, `config` (in-memory registry + ADR 0027 contribution API — no flat-TOML loader by design), `client`, `pool`, `server` (inbound JSON-RPC), `transport`.
- `on_init` is a near no-op (debug-log only) — the registry starts empty. Contributions populate it post-load via `crates/nexus-bootstrap/src/acp_contribution_wiring.rs`. `on_start` publishes `com.nexus.acp.started` with `registered_agents` count. `on_stop` matches the LSP/DAP 5s-capped pool shutdown.
- Persistence: none on disk for the host (no `acp.toml`). The header at `docs/0.1.2/settings/forge-config.md:110` is "lsp.toml / dap.toml / acp.toml" but ACP is greenfield-contribution-only per ADR 0027 §Phase 4 — `acp.toml` is reserved for forward compatibility but not parsed today.
- External dependencies: spawns arbitrary child processes (agent `command`s), reads/writes stdio. No network from this crate. `tokio`, `tracing`, `serde_json`, `thiserror`. Notably **does not** depend on `toml` (no flat-TOML loader).

## Surface

- IPC handlers (from `IPC_HANDLERS` in `core_plugin.rs:63`):
  - `list_agents` (1) — sync, registry shape with `connected: false` stub.
  - `initialize` (2) — async, forces lazy connect + returns `capabilities`.
  - `propose` (3), `accept` (4), `reject` (5) — async, proposal lifecycle.
  - `register_server` (6), `unregister_server` (7) — sync, BL-113 Phase 4 contributions (skip reasons: `already_registered`, `invalid_name`, `invalid_command`, `not_owned_by_plugin`).
  - `disconnect` (8) — async, drop the agent connection (pool can re-establish next call).
- Bus events: agent notifications fan out as `com.nexus.acp.<method-with-dots>` (e.g. `agent/output` → `com.nexus.acp.agent.output`).

## Necessity

- **Verdict:** Optional
- **Required for basic capabilities?** No. Agent-protocol routing is not needed for markdown edit / search / git.
- **Depended on by:** the inbound `AcpServer` is invoked by the `nexus acp serve` CLI subcommand in `crates/nexus-cli/src/app.rs`. First-party example agent at `plugins/first-party-acp-echo/`. **No shell UI plugin consumes the outbound host today** — `shell/src/types/pluginIds.ts` references the id but no `shell/src/plugins/nexus/acp/` directory exists. Bootstrap contribution wiring + `acp_contribution_wiring` integration tests are the only in-tree consumers of the host's register/list verbs.
- **Depends on:** `nexus-kernel`, `nexus-plugins`. The server side ultimately routes to `com.nexus.agent` via the kernel.
- **What breaks if removed:** the `nexus acp serve` CLI verb stops working; the `first-party-acp-echo` example can't be hosted. BL-113 Phase 4 contributed ACP agents would have nowhere to register. Markdown / search / git unaffected.

## Notes

- **Surprise:** the outbound host has no shell-side consumer in tree. The plugin id appears in `shell/src/types/pluginIds.ts` but there is no `shell/src/plugins/nexus/acp/` directory and no IPC calls from shell code. The only inbound traffic to `com.nexus.acp::register_server` / `list_agents` today is the bootstrap contribution wiring and its integration tests. The inbound `AcpServer` (`nexus acp serve`) is the only "live" user-facing entry point.
- Same BL-113 verb-level capability caveat as LSP/DAP.
- Hard-coded 5s shutdown deadline.
- Lighter test coverage than LSP/DAP (the bulk of behavioural tests live in `nexus-bootstrap`'s `acp_contribution_wiring.rs` integration test).
- Greenfield-only contribution surface — ADR 0027 §Phase 4 explicitly skips the flat-TOML deprecation window other host plugins inherited.
