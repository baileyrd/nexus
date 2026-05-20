# com.nexus.mcp.host

- **Path:** `crates/nexus-mcp/`
- **Tier:** Core Rust
- **Bootstrap order:** 18

## Architecture
- Entry point: `crates/nexus-mcp/src/core_plugin.rs` (`McpHostPlugin`). Modules: `client`, `server`, `config`, `pool`, `auth`, `dynamic_tools`, `ipc`.
- Two halves of the MCP protocol live in this crate:
  - **Host client** — Nexus connects *to* external MCP servers listed in `<forge>/.forge/mcp.toml`. The plugin exposes connection management + tool/resource/prompt invocation over IPC.
  - **`NexusMcpServer`** — exposes forge ops *to* external AI clients (Claude Desktop, Cursor, Cline). Spawned separately by the `nexus-mcp` CLI binary; not registered via `McpHostPlugin`.
- Connections are lazy: the first IPC call targeting a server triggers `ConnectionPool::connect` and the rmcp handshake; subsequent calls reuse the pooled connection. Avoids blocking startup on slow servers.
- Lifecycle: `LifecycleFlags { on_init: true, on_start: true, on_stop: true }` (`crates/nexus-bootstrap/src/plugins/mcp.rs:27`). `on_stop` drains the pool.
- BL-113 plugin-contributed servers: `register_server` / `unregister_server` mutate the `Arc<RwLock<McpHostConfig>>` so dynamic-tool registration can advertise the contributing plugin.
- DG-39 / PRD-14 §10 `DynamicToolRegistry` lets plugins publish callable tools that `NexusMcpServer` re-exposes outward.

## Persistence
- `<forge>/.forge/mcp.toml` — `McpHostConfig` (`crates/nexus-mcp/src/config.rs:144`). Documented in `docs/0.1.2/settings/forge-config.md` lines 85–108.
- No SQLite, no sidecars. `[contributed_by]` map is runtime-only.

## Settings owned
- `[servers.<name>]` — command / args / env / working_dir / transport (`stdio`, `streamable-http` with `url`).
- `[timeouts]` — `connect_secs` (15), `shutdown_secs` (5), `ipc_secs` (30), `ai_ipc_secs` (120), `oauth_secs` (30). Defaults at `nexus_mcp::{client,server,auth}::DEFAULT_*`.

## External dependencies of note
- `rmcp` (workspace) — wraps the official MCP SDK; provides `transport-streamable-http-client-reqwest`.
- `reqwest` for the in-house OAuth client-credentials token fetcher (`auth.rs`).
- `http` for typed header parsing on Streamable HTTP transport.
- Child-process spawning for `stdio` transport servers.

## Surface
Handlers (`IPC_HANDLERS`, `src/core_plugin.rs:81`):

| Id | Command | Notes |
|---:|---------|-------|
| 1 | `list_servers` | Configured names + transport |
| 2 | `list_tools` | Lazy connect then enumerate (async) |
| 3 | `call_tool` | `{ server, tool, arguments }` (async) |
| 4 | `list_resources` | Async |
| 5 | `list_prompts` | Async |
| 6 | `connect` | Force connect (async) |
| 7 | `disconnect` | Drop pooled connection (async) |
| 8 | `register_tool` | DG-39 dynamic tool register |
| 9 | `unregister_tool` | DG-39 dynamic tool unregister |
| 10 | `list_dynamic_tools` | Enumerate dynamic registry |
| 11 | `register_server` | BL-113 plugin-contributed server |
| 12 | `unregister_server` | BL-113 plugin-contributed server |

Publishes: `com.nexus.mcp.host_started` (one-shot snapshot consumed by workflow `mcp_event` triggers).

## Necessity
- **Verdict:** Optional
- **Required for basic capabilities?** No — opening, browsing, editing, searching, and committing markdown does not consult external MCP servers.
- **Depended on by:** `com.nexus.agent` (agents call tools), `com.nexus.workflow` (`mcp_event` trigger + step types), the `NexusMcpServer` binary (when exposing forge ops outward), shell `mcp` feature plugin.
- **Depends on:** child-process spawning, outbound network (HTTP transport), OAuth provider (when configured).
- **What breaks if removed:** agent access to external tools, workflow `mcp_event` triggers, plugin-contributed MCP servers, the outward-facing `nexus-mcp` server CLI. None of these break the basic markdown workflow.

## Notes
- Largest plugin in this batch by surface (12 handlers + 9 modules). Lazy connection design means an empty / absent `mcp.toml` is a no-op at boot.
- Two `reqwest` versions in the dep graph (rmcp's pinned version vs. workspace) are deliberately not bridged at the type layer — OAuth tokens cross as plain `Bearer …` strings (`Cargo.toml:20`).
- The crate also houses `NexusMcpServer` (forge-ops-outward), which is *not* part of `McpHostPlugin`'s IPC surface — the server is launched by a separate binary that imports the crate directly.
