# nexus-mcp

> Kind: lib · IPC plugin id: com.nexus.mcp.host · CorePlugin: yes · Has settings: McpHostConfig · As of: 2026-05-25

## Overview

`nexus-mcp` is the crate's two-sided implementation of the Model Context Protocol, both built on `rmcp`. The **server** half ([`NexusMcpServer`]) exposes Nexus forge operations — note CRUD, search, link graph, tags, tasks, RAG, skills, and BL-114/BL-115 code-intel — as MCP tools to external AI clients (Claude Desktop, Cursor, Cline, …). The **host/client** half ([`McpClient`], driven by [`McpHostPlugin`]) lets Nexus itself act as an MCP host: it reads `<forge>/.forge/mcp.toml`, connects to the external MCP servers listed there, and re-exposes their tools/resources/prompts over IPC so other plugins (notably the AI tool bridge) can call them without linking `rmcp`. The same wire protocol carries both halves; only the role differs.

The server is **not** a separate crate binary — `nexus-mcp` has no `[[bin]]`. It runs as `nexus mcp serve` (`crates/nexus-cli/src/commands/mcp.rs`), which builds a CLI runtime, constructs `NexusMcpServer::new(context)`, and calls `serve_stdio()`. Every tool call routes through `context.ipc_call(...)` into `com.nexus.storage`, `com.nexus.ai`, `com.nexus.git`, `com.nexus.security`, and `com.nexus.skills`, so each operation is capability-checked and audited at the kernel — the MCP server holds no privileged file/AI access of its own. Tool calls and resource reads are audited via `nexus_kernel::audit::log_mcp_tool_call` / `log_mcp_resource_read` (DG-40), and per-call responses are size-capped (4 MiB / 1024 items, issue #85). C46 (#399) — `serve` also spawns `nexus_bootstrap::dream_cycle::spawn` for the process's lifetime (stopped after `serve_stdio()` returns), matching the TUI and shell — previously only the TUI ran the BL-129 background maintenance scheduler, so an MCP-only session got no dedup/decay/enrich/infer/extract.

The host side is wired as the `com.nexus.mcp.host` core plugin (registered by `nexus-bootstrap`). Connections are **lazy** — established on the first IPC call that targets a server, never eagerly at startup — and managed by a [`ConnectionPool`] with idle eviction and transient-failure reconnect/backoff. Three transports are modelled: stdio (default, child process), Streamable HTTP (BL-023; POST + SSE under one endpoint, the 2025-03-26 spec), and WebSocket (reserved in config but not dispatchable — rmcp 1.3/1.5 ships no WS impl; connect returns a clear "use http" error). Remote (`http`) servers support BL-025 auth: static API key, static bearer, or OAuth 2.0 client-credentials (token fetched at connect time).

The dynamic tool registry (DG-39 / PRD-14 §10) is a process-global table letting any plugin publish its own MCP tool at runtime via the `register_tool` IPC verb; the server's `list_tools` returns the union of the static `nexus_*` set plus dynamic entries, and `call_tool` checks the dynamic registry first, routing hits back through `ipc_call(plugin_id, command, args)`. This fits the microkernel model: `nexus-mcp` depends only on `nexus-kernel` / `nexus-plugins` / `nexus-types` and never the reverse; every capability flows through IPC.

## Position in the dependency graph

- **Direct nexus-\* deps:** `nexus-kernel` (`KernelPluginContext`, `Ipc`, `EventBus`, `audit`), `nexus-plugins` (`CorePlugin`, `PluginError`, `CorePluginFuture`), `nexus-types` (`plugin_ids`, `IPC_TIMEOUT_SHORT/LONG`).
- **Notable external deps:** `rmcp` 1.3 (features: `server`, `client`, `transport-io`, `transport-child-process`, `transport-streamable-http-client`, `transport-streamable-http-client-reqwest`, `reqwest-native-tls`, `client-side-sse`, `macros`); `http` 1 (typed header parsing for the Streamable HTTP transport, BL-023); `reqwest` (workspace 0.12, used **only** by the in-house OAuth token fetcher in `auth.rs`, BL-025 — deliberately separate from rmcp's own pinned reqwest); `schemars` (tool input/output JSON Schemas, unconditional); `serde`/`serde_json`/`toml`/`thiserror`/`tracing`/`tokio`; optional `ts-rs` behind the `ts-export` feature.
- **Crates depending on it:** `nexus-bootstrap` (registers the host plugin), `nexus-cli` (runs `NexusMcpServer` via `nexus mcp serve`), `nexus-dap` and `nexus-lsp` (reuse the BL-113 `merge_contributed` / contribution-wiring pattern shape; they link the crate for the shared config types).

## Public API surface

**`lib.rs`** — re-exports the supported surface: `McpClient`/`McpClientError`, `McpHostConfig` + config types (`McpServerSpec`, `McpTransport`, `McpConfigError`, `McpMergeSkip`, `McpMergeSkipReason`, `McpUnregisterError`), `McpHostPlugin`, `DynamicTool`/`DynamicToolRegistry`/`ToolRegistryError`, `ConnectionPool`/`PoolConfig`, `NexusMcpServer`, and auth types (`McpAuth`, `McpAuthSecret`, `ResolvedAuth`, `AuthError`).

**`server.rs`** — `NexusMcpServer`: `new(Arc<KernelPluginContext>)`, `serve_stdio()`; the `#[tool_router]` impl carrying the static tools; `impl rmcp::ServerHandler` (`get_info`, `call_tool`, `list_tools`, `list_resources`, `read_resource`). Helpers: `parse_note_uri`, `build_note_resource`, `storage_call`/`skills_call`/`git_call`/`security_call`, `query_symbol_rows`, `build_symbol_context`, `risk_for_kind`, `dynamic_tool_to_rmcp`. Per-tool input/output structs each derive `schemars::JsonSchema`.

**`client.rs`** — `McpClient`: one live connection to an external server. `connect(name, spec)` dispatches per transport (`connect_stdio` / `connect_http`; websocket → `Unsupported`); `name`, `list_tools`, `list_resources`, `list_prompts`, `call_tool`, `shutdown`. `McpClientError` with `is_transient()` (only `Service` is transient). Constants `DEFAULT_CONNECT_TIMEOUT` (15 s) / `DEFAULT_SHUTDOWN_TIMEOUT` (5 s).

**`config.rs`** — `McpHostConfig` (`servers: BTreeMap`, `timeouts`, runtime-only `contributed_by`); parsing (`from_str`, `read_from`), `enabled_servers()`, BL-113 mutation API (`merge_contributed`, `register_contributed`, `unregister_contributed`). `McpServerSpec`, `McpTransport` enum, `McpTimeouts`, error/skip enums.

**`core_plugin.rs`** — `McpHostPlugin` (`new(forge_root, Option<Arc<EventBus>>)`), the `CorePlugin` impl, the `PLUGIN_ID` const, per-handler `HANDLER_*` id consts, and `IPC_HANDLERS: &[(&str, u32)]` (the SD-06 single source of truth consumed by bootstrap).

**`dynamic_tools.rs`** — `DynamicTool` declaration; `DynamicToolRegistry` (`register`/`unregister`/`lookup`/`list`/`len`/`is_empty`); `RegistryError`; `global()` process-wide `OnceLock` accessor; `validate_name` (rejects empty + `nexus_` prefix).

**`auth.rs`** — `McpAuth` (tagged enum: `ApiKey`/`Bearer`/`OauthClientCredentials`), `McpAuthSecret`/`ClientIdSecret`/`ClientSecretSecret` (inline-or-env untagged enums), `ResolvedAuth` (final headers), `AuthError`, async `resolve()`, `DEFAULT_OAUTH_TIMEOUT` (30 s).

**`pool.rs`** — `ConnectionPool` (`new`, `get_or_connect`, `disconnect`, `shutdown_all`, `sweep_idle`, `call_with_reconnect`); `PoolConfig` (`max_per_server`, `idle_timeout`, `connect_timeout`, `backoff`); `default_backoff()`. Internal `Connectable` test-seam trait.

**`ipc.rs`** — wire-mirror IPC arg/reply structs (audit P1-3 #113) for schema/TS generation: `McpServerArgs`, `McpCallToolArgs`, `McpServerEntry`, `McpToolEntry`, `McpResourceEntry`, `McpPromptEntry`, `McpConnectReply`, `McpDisconnectMissReply`, `McpCallToolReply`, `McpRegisterServerArgs`/`McpRegisterServerReply`, `McpUnregisterServerArgs`/`McpUnregisterServerReply`. These mirror the runtime `json!` shapes; the handlers themselves still build responses ad hoc.

## IPC handlers

Host plugin `com.nexus.mcp.host`. Sync handlers run in `dispatch`; async handlers (those needing a live connection) run in `dispatch_async`. Args missing a required `server` field make `dispatch_async` return `None`.

| # | command | args | returns | capability | description |
|---|---------|------|---------|------------|-------------|
| 1 | `list_servers` | — | `[{name, command, args, disabled}]` | — | enumerate configured servers (sync) |
| 2 | `list_tools` | `{server}` | `[{name, description, input_schema}]` | — | connect-if-needed, list server tools (async) |
| 3 | `call_tool` | `{server, tool, arguments?}` | `{content[], is_error, truncated}` | — | invoke a tool; response size-capped to 4 MiB/1024 items (async). Tool side effects run in the server's process |
| 4 | `list_resources` | `{server}` | `[{uri, name, description, mime_type}]` | — | list server resources (async) |
| 5 | `list_prompts` | `{server}` | `[{name, description}]` | — | list server prompts (async) |
| 6 | `connect` | `{server}` | `{ok, server}` | `process.spawn` (stdio) / `net.connect` (http) | explicitly establish a connection (async) |
| 7 | `disconnect` | `{server}` | `{ok, server}` or `{ok:false, server, reason}` | — | tear down a connection (async) |
| 8 | `register_tool` | `DynamicTool` (`{name, description, input_schema, plugin_id, command}`) | `{ok:true}` | — | DG-39 register a dynamic tool (sync); rejects empty / `nexus_`-prefixed names and duplicates |
| 9 | `unregister_tool` | `{name}` | `{removed, name}` | — | DG-39 remove a dynamic tool (sync) |
| 10 | `list_dynamic_tools` | — | `[DynamicTool]` | — | DG-39 list registry entries (sync) |
| 11 | `register_server` | `{name, transport?, command?, args?, env?, url?, disabled?, plugin_id}` | `{ok, status, reason?}` | invoker-only (`protocol.host.contribute`; not gated at verb level — ADR 0027) | BL-113 register a plugin-contributed external server (sync); status ∈ `ok`/`toml_override`/`invalid_name`/`invalid` |
| 12 | `unregister_server` | `{name, plugin_id}` | `{ok, status, actual_owner?}` | invoker-only (`protocol.host.contribute`) | BL-113 remove a contributed server (sync); status ∈ `ok`/`not_found`/`toml_entry`/`not_owned_by_plugin` |

Capability notes: the crate performs **no** capability checks itself — gating lives at the kernel IPC boundary (manifest-declared) and at the `connect` boundary for spawn/network. `register_server`/`unregister_server` deliberately have no verb-level gate (the bootstrap wiring helper is the only intended caller; hard enforcement is a filed follow-up). The capabilities column reflects `docs/0.1.2/ipc-handlers.md` / `capabilities.md`, not in-crate code.

## MCP tools exposed

The server presents 19 static `nexus_*` tools (the `#[tool]` macros in `server.rs`) plus any dynamically-registered tools, to external MCP clients. (The `server.rs` module doc's "15 tools" headline is stale.) Each routes through `ipc_call`; on IPC failure most tools return a degraded payload rather than a protocol error.

| tool | input (summary) | forge op |
|------|-----------------|----------|
| `nexus_read_note` | `{path}` | storage `read_file` → content + size |
| `nexus_create_note` | `{path, content}` | storage `write_file` |
| `nexus_update_note` | `{path, content}` | storage `write_file` (creates if absent) |
| `nexus_delete_note` | `{path}` | storage `delete_file` |
| `nexus_list_notes` | `{prefix?}` | storage `query_files` |
| `nexus_search` | `{query, limit?=20}` | storage `rebuild_search_index` then `search` (Tantivy FTS) |
| `nexus_backlinks` | `{path}` | storage `backlinks` |
| `nexus_outgoing_links` | `{path}` | storage `outgoing_links` |
| `nexus_graph_status` | — | storage `graph_stats` (node/edge/unresolved counts) |
| `nexus_list_tags` | `{name}` | storage `query_tags` |
| `nexus_list_tasks` | `{completed?, file?}` | storage `query_tasks` |
| `nexus_toggle_task` | `{task_id}` | storage `toggle_task` |
| `nexus_ask` | `{question}` | AI `ask` (RAG over indexed notes; long timeout) |
| `nexus_list_skills` | — | skills `list` (authored prompt templates from `.forge/skills/`) |
| `nexus_render_skill` | `{id, values?}` | skills `render` (template expansion) |
| `nexus_context` | `{name, path?}` | BL-115: storage `query_symbol` → symbol location, doc, parent, siblings |
| `nexus_impact` | `{name, path?, depth?}` | BL-115: kind-based risk band + sibling-proxy "direct callers"; `degraded` always true |
| `nexus_detect_changes` | — | BL-115: git `file_statuses` joined against indexed symbols per dirty file |
| `nexus_kernel_stats` | — | BL-137: security `metrics_snapshot` (kernel BL-093 metrics; read-only) |

The BL-115 tools always set `degraded: true` with a fixed `degraded_reason` because the BL-114 index records declarations only (no call-edge traversal yet). The server also exposes forge notes as **MCP resources** under `mcp://nexus/notes/<path>` (`list_resources` via storage `query_files`; `read_resource` via storage `read_file`).

## Capabilities

`nexus-mcp` declares and checks no capabilities in its own code. All gating is external:

- IPC handler caps are declared in the plugin manifest (`docs/0.1.2/capabilities.md` / `ipc-handlers.md`): `connect` → `process.spawn` (stdio) / `net.connect` (http); `register_server`/`unregister_server` → `protocol.host.contribute` (invoker-only, not enforced at verb level per ADR 0027); all other host verbs are ungated read/registry operations.
- The MCP **server** holds no privileged access: every tool/resource op is an `ipc_call` into a service plugin, so the kernel applies that plugin's capability checks and audits the call.
- `call_tool` runs the actual side effects inside the external server's own process; the `connect`-time `process.spawn`/`net.connect` gate bounds who can attach a server at all.

## Settings / Config

`McpHostConfig` is parsed from `<forge>/.forge/mcp.toml` at `on_init`. A missing file is not an error (equals "no external servers"). Top-level fields:

- `servers: BTreeMap<String, McpServerSpec>` — name → spec. `BTreeMap` keeps order stable across writes.
- `timeouts: McpTimeouts` (`[timeouts]`) — P2-06 per-forge overrides: `connect_secs`, `shutdown_secs`, `ipc_secs`, `ai_ipc_secs`, `oauth_secs`. **Parsed but not yet threaded through** — the `DEFAULT_*` consts remain the operational defaults until the call sites are refactored.
- `contributed_by: HashMap<String, String>` — runtime-only (`#[serde(skip)]`); maps contributed server name → owning plugin id for unregister authorization. TOML entries never appear here.

`McpServerSpec` fields (all `serde(default)`):

- `transport: McpTransport` — `stdio` (default), `http`, or `websocket` (reserved). Lowercase serde rename.
- `command` / `args` / `env` — stdio fields (executable, argv, env map merged into the child).
- `url` — required for `http`/`websocket`; ignored for stdio.
- `auth_header` — static raw `Authorization` value (BL-023 fast path).
- `headers: BTreeMap` — custom HTTP headers (Streamable HTTP only).
- `auth: Option<McpAuth>` — BL-025 declarative auth (http only); resolver output overrides static `auth_header` on conflict.
- `disabled: bool` — keep the entry but skip it at connect time.

Validation: stdio needs a non-empty `command`; remote transports need a non-empty `url`.

`mcp.toml` structure example:

```toml
[servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
[servers.filesystem.env]
NODE_ENV = "production"

[servers.remote]
transport = "http"
url = "https://example.com/mcp"
[servers.remote.headers]
X-Workspace = "alpha"
[servers.remote.auth]               # BL-025; one of three flows
type = "bearer"
token = "ey…"                       # or: env = "BETA_TOKEN"
# type = "api_key" / header = "X-API-Key" / value | env
# type = "oauth_client_credentials" / token_url / client_id[_env] / client_secret[_env] / scope?

[timeouts]
connect_secs = 20
```

`PoolConfig` (not from TOML; constructed with defaults): `max_per_server = 10`, `idle_timeout = 300 s`, `connect_timeout = 30 s`, `backoff = [100 ms, 500 ms, 2 s, 10 s, 30 s]` (PRD-14 §11.1).

## Events

- **Published:** `com.nexus.mcp.host.started` on `on_start` with `{configured_servers: N}` (best-effort via the injected `EventBus`; a publish failure is logged, not fatal).
- **Subscribed:** none.

The server side does not publish/subscribe events; it emits audit records (`log_mcp_tool_call`, `log_mcp_resource_read`) instead.

## Internals & notable implementation details

**Server request handling.** `NexusMcpServer` holds an `Arc<KernelPluginContext>` and a generated `ToolRouter<Self>`. `call_tool` first checks the DG-39 global registry: a dynamic hit is dispatched through `ipc_call(plugin_id, command, args)` and wrapped as a `structured` result; otherwise the static router handles it. Both paths are timed and audited (DG-40) with success/error + duration. Static tools can never collide with dynamic ones because `validate_name` reserves the `nexus_` prefix. Two IPC timeout bands apply: `IPC_TIMEOUT_SHORT` for storage/git/security/skills, `IPC_TIMEOUT_LONG` for the AI `ask` call.

**Dynamic tool registration (DG-39).** The registry is a process-global `OnceLock<Arc<DynamicToolRegistry>>` (same pattern as `nexus_kernel::audit_store`) so the `McpHostPlugin::dispatch` handlers and the separately-constructed `NexusMcpServer` share state without threading an Arc through bootstrap. Interior `RwLock<BTreeMap>`; `list_tools` appends dynamic entries alphabetically after the static set; re-registration after unregister is allowed (supports hot-reload of tool metadata).

**External-server connection lifecycle.** `McpHostPlugin` keeps the config behind `Arc<RwLock<McpHostConfig>>` (so `register_server`/`unregister_server` can mutate at runtime) and a shared `Arc<ConnectionPool>`. Async dispatchers snapshot the config per-future so an in-flight command keeps a stable server view even if the master config mutates underneath. `on_stop` drops the pool on a dedicated current-thread tokio runtime with a 5 s hard-cap poll loop (issue #85): a child that ignores the graceful close is abandoned (OS reclaims at process exit) rather than blocking kernel shutdown.

**Connection pool.** One `Arc<Mutex<McpClient>>` per server, created lazily. `get_or_connect` sweeps idle entries (lazy, no background task — `last_used` older than `idle_timeout` evicted on next access) before fetching. `call_with_reconnect` retries `Service` (transient) failures against the backoff schedule, force-reconnecting between attempts; `Spawn`/`Handshake` (non-transient) bail immediately. A per-entry `Semaphore` caps concurrent in-flight calls (advisory today, since the inner `Mutex` already serializes). A `Connectable` trait seam lets tests inject a fake connector without spawning real processes.

**Transport selection (BL-023).** `McpClient::connect` matches on `spec.transport`. Stdio spawns the command with piped stdin/stdout and inherited stderr (so server startup logs reach the operator's terminal) via `TokioChildProcess`. HTTP builds a `StreamableHttpClientTransport` from a `StreamableHttpClientTransportConfig`, validating custom header names/values up front and merging BL-025-resolved auth before construction. Both paths share `run_handshake`, which races `serve_client(...)` against the 15 s `CONNECT_TIMEOUT`. Websocket returns `Unsupported` pointing at `http`. A deliberate reqwest-version split is documented: the HTTP transport uses rmcp's own pinned reqwest (named only via `from_config` to dodge naming the type), while `auth.rs`'s OAuth fetcher uses the workspace `reqwest 0.12` — safe because the token crosses the rmcp boundary as a plain `Bearer …` string.

**Auth resolution (BL-025).** `auth::resolve` reduces every flow to a `ResolvedAuth { authorization, extra_headers }`. `ApiKey` on `Authorization` lands on the dedicated rmcp auth slot; on any other header it goes to extras. `Bearer` prepends `Bearer ` if absent. `OauthClientCredentials` does one HTTP POST (`grant_type=client_credentials`, HTTP Basic creds, optional scope) with a 30 s timeout, requires a `bearer` token_type and non-empty `access_token`; no refresh-on-401 (refetched on next connect). Secrets are inline or env-indirected; a missing/empty env var fails fast as `AuthError::MissingEnv` before any transport is built. ADR-0009 keyring variants can slot in without a wire break.

**BL-113 contribution model (ADR 0027).** `register_contributed` validates the name + per-transport rules, refuses any existing name (TOML or plugin), inserts, and records provenance in `contributed_by`. TOML wins on collision (`TomlOverride`). `unregister_contributed` gates on owner match: TOML-pinned entries (`TomlEntry`), wrong owner (`NotOwnedByPlugin { actual_owner }`), or unknown (`NotFound`). `merge_contributed` is the batch form used by `nexus-bootstrap`'s `mcp_contribution_wiring`.

## Tests

All tests are inline `#[cfg(test)]` modules — there is no `tests/` directory.

- **`config.rs`** — TOML parsing (minimal, env+disabled, empty file, empty-command rejection, missing file), `enabled_servers` filtering, serialize roundtrip; BL-023 transport variants (http parse + url-required, stdio back-compat default, websocket reserved); BL-113 `merge_contributed`/`register_contributed`/`unregister_contributed` (insert, TOML-wins, invalid specs, input-order preservation, `contributed_by` provenance, owner-match removal, all skip reasons).
- **`core_plugin.rs`** — plugin id, `on_init` with/without `mcp.toml`, `list_servers` empty/populated, async-handler `None` on missing arg, unknown-handler error, `on_start`/`on_stop` safety; DG-39 register/unregister/list + reserved-prefix rejection; BL-113 `register_server`/`unregister_server` IPC round-trips and every skip status.
- **`client.rs`** — spawn-failure for a nonexistent binary, handshake timeout against a silent process (Linux); BL-023 dispatch: websocket→Unsupported, http missing-url→Config, invalid header→Config, BL-025 missing-env→Auth, dead-endpoint handshake.
- **`auth.rs`** — ApiKey on Authorization vs custom header, bearer scheme prepend/preserve, env resolve set/unset, TOML parsing of api_key + oauth_client_credentials shapes.
- **`pool.rs`** — backoff schedule matches spec, default config, `call_with_reconnect` exhausts schedule / skips non-transient, idle sweep on empty pool, disconnect/shutdown on empty pool, `is_transient` variants. Uses a `FakeConnector` seam since a real `McpClient` needs a live transport.
- **`dynamic_tools.rs`** — register/lookup, duplicate/reserved/empty rejection, unregister-then-reregister, alphabetical list, `global()` pointer identity.
- **`server.rs`** — `parse_note_uri`, `build_note_resource` (uri/mime/size, u32 clamp), skill input/output (de)serialization, BL-115 `risk_for_kind` bands, `QuerySymbolRow`/`QuerySymbolReply` decode, output struct serialization, degraded-reason content.
- **`ipc.rs`** — `McpToolEntry` round-trips `input_schema` and decodes when it is absent (G5a AI-bridge contract).

## Gaps / caveats

- `McpTimeouts` is parsed but **not wired** — the `DEFAULT_*` constants are still the live values.
- `register_server`/`unregister_server` have no enforced verb-level capability gate yet (documented ADR 0027 follow-up).
- WebSocket transport is config-accepted but never dispatchable.
- The `server.rs` module doc says "15 tools"; the actual static count is 19.
- `ipc.rs` doc comments reference a `[mcp.servers.<name>]` table under `config.toml`; the live config is the dedicated `<forge>/.forge/mcp.toml` `[servers.<name>]` table — treat the `ipc.rs` wording as stale.
- No integration test connects to a real external MCP server (would require a second binary); the pool/client success paths are exercised only via fakes/error paths.
