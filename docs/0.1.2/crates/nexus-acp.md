# nexus-acp

> Kind: lib · IPC plugin id: com.nexus.acp · CorePlugin: yes · Has settings: AcpHostConfig · As of: 2026-05-25

## Overview

**ACP = Agent Client Protocol.** Per the crate source, ACP is a JSON-RPC 2.0 protocol for driving "agent" processes — LLM-backed sub-processes that expose verbs like `initialize`, `propose`, `accept`, `reject` and push fire-and-forget notifications such as `agent/output`. The wire framing is **newline-delimited JSON** (one JSON-RPC 2.0 message per line, terminated by `\n`) rather than LSP's `Content-Length:` header framing. The lib comment justifies this choice as matching the "Hermes Feature-7" wire shape and most JSON-RPC tooling defaults, and keeping the transport debuggable from a terminal. A 16 MiB per-line ceiling (`MAX_LINE_BYTES`) caps memory so a misbehaving peer can't OOM the host.

This single crate plays **two complementary roles**:

1. **Host (outbound, BL-144 / ADR 0027 Phase 4).** The `AcpCorePlugin` core plugin (id `com.nexus.acp`) spawns ACP-speaking agent sub-processes that are *declared by community plugins* via the manifest contribution point `[[registrations.protocol_hosts.acp]]`. It proxies request/response traffic over IPC and republishes agent-pushed notifications on the kernel event bus as `com.nexus.acp.<method-with-dots>`. The crate doc states it mirrors `nexus-lsp` "layer for layer," with the deliberate difference that **there is no `acp.toml` flat-TOML loader** — ADR 0027 §Phase 4 lands ACP greenfield under the contribution model, so all adapters arrive through `register_server` IPC calls at plugin-load time.

2. **Server (inbound, BL-145 / Hermes Feature 7).** `AcpServer` is a line-delimited JSON-RPC 2.0 stdio surface that exposes a *fixed allow-list* of Nexus's own `com.nexus.agent` IPC verbs to external Hermes-compatible parent processes. It is a pure proxy with no kernel-context borrow beyond `PluginContext::ipc_call`. The only intended caller is the `nexus acp serve` CLI subcommand, which wires `stdin`/`stdout` so a parent process can drive Nexus's agent loop.

**Microkernel fit.** The crate honors invariant #3 (IPC over direct calls): the inbound server routes every method through `context.ipc_call(plugin_id, command, args, timeout)` against the kernel rather than reaching into agent internals, and the outbound host is itself a `CorePlugin` registered by `nexus-bootstrap`. Adapters are populated at runtime through the `register_server` IPC verb dispatched by the bootstrap contribution wiring; the registry starts empty by design. **Status note (from `core_plugin.rs`):** as of 0.1.2 the host is *experimental* — its IPC surface is fully wired and unit-tested, but no in-tree shell plugin invokes `com.nexus.acp::*`. The only user-facing entry points are the inbound `nexus acp serve` subcommand and the `first-party-acp-echo` example plugin.

## Position in the dependency graph

- **Direct nexus-\* deps:** `nexus-kernel` (for `EventBus`, `KernelPluginContext`, `Ipc`, `PluginContext::ipc_call`), `nexus-plugins` (for `CorePlugin`, `CorePluginFuture`, `PluginError`). Notably it does **not** depend on `nexus-types` directly, and the config layer deliberately re-defines `AcpAdapterSpec` rather than importing `nexus_plugins::manifest::AcpProtocolHostReg`, so the IPC handlers and connection pool don't pull in `nexus-plugins` types.
- **Notable external deps:** `tokio` (async process spawning, pipes, channels, timeouts), `serde` / `serde_json` (JSON-RPC envelopes), `schemars` (JSON Schema export of wire-mirror IPC types), `thiserror` (error enums), `tracing`. `ts-rs` is optional behind the `ts-export` feature (mirrors nexus-lsp / nexus-dap) so the production build doesn't pull it.
- **Crates depending on it:** `nexus-bootstrap` (registers the core plugin in `src/plugins/acp.rs`, wires contributions in `acp_contribution_wiring.rs`, converts manifest contributions to specs in `protocol_host_specs.rs`) and `nexus-cli` (the `acp serve` subcommand in `src/commands/acp.rs`).

## Public API surface

| Module | Re-exports | Purpose |
|--------|-----------|---------|
| `transport` (private mod) | — | Line-delimited JSON-RPC 2.0 framing over stdio. Defines `JsonRpcMessage` (untagged enum: `Request` / `Response` / `Notification`), `JsonRpcRequest` / `JsonRpcResponse` / `JsonRpcError` / `JsonRpcNotification` envelopes, `TransportError`, `MAX_LINE_BYTES` (16 MiB), and the async `read_message` / `write_message` framing functions. Shared by both directions. |
| `client` (private mod) | `AcpClient`, `AcpClientError` | One client = one running agent child process. Spawns the executable, runs the ACP `initialize` handshake, demultiplexes inbound responses/notifications via a reader task, and exposes `send_request` / `send_notification` / `drain_notifications` / `shutdown`. |
| `config` (pub mod) | `AcpAdapterSpec`, `AcpHostConfig`, `AcpConfigError`, `MergeSkip` (as `AcpMergeSkip`), `MergeSkipReason` (as `AcpMergeSkipReason`), `UnregisterError` (as `AcpUnregisterError`) | In-memory adapter registry + BL-113 contribution API. No `acp.toml`. |
| `pool` (pub mod) | `ConnectionPool`, `PoolConfig` | Lazy-connect pool keyed by agent name; exponential-backoff reconnect on transient failure; graceful `shutdown_all`. Also `default_backoff()`. |
| `server` (pub mod) | `AcpServer`, `AcpServerError` | Inbound JSON-RPC stdio surface. Also pub: `route_method`, `RoutedMethod`, `invalid_params_response`, `DEFAULT_DISPATCH_TIMEOUT`. |
| `core_plugin` (pub mod) | `AcpCorePlugin` | The `com.nexus.acp` core plugin. Also pub: `PLUGIN_ID`, the `HANDLER_*` id constants, and `IPC_HANDLERS` (the SD-06 single source of truth for `(command-name, handler-id)` pairs). |
| `ipc` (pub mod) | — | Wire-mirror serde/schemars/ts-rs structs for the schema generator and `scripts/check_ipc_drift.sh`. |

## IPC handlers

Eight handlers, defined in `core_plugin::IPC_HANDLERS`. Three are sync (`dispatch`), five are async (`dispatch_async`). The capability column reflects the manifest/kernel gate documented in `docs/0.1.2/ipc-handlers.md` — **the crate source itself does no verb-level capability check**; the register/unregister handlers carry the comment "no verb-level capability gate" and runtime authorisation rides on the kernel capability matrix attached to the calling plugin plus the `contributed_by` ownership check.

| Handler id | command | args | returns | capability | description |
|---|---|---|---|---|---|
| 1 | `list_agents` | — (ignores args) | JSON array of `{name, command, args, capabilities, disabled, connected, metadata}` (one per registered adapter) | — | Sync. Lists configured adapters from the registry. `connected` is hardcoded `false` — the sync handler can't await the async `pool.connected_agents()`; a real "connected" column awaits an async list variant if a use case appears. |
| 2 | `initialize` | `{agent}` (`AcpAgentArgs`) | `{agent, capabilities}` | `process.spawn` | Async. Forces a lazy connect (spawning the child + running the `initialize` handshake) and returns the agent-reported capabilities object. |
| 3 | `propose` | `{agent, action, params?}` (`AcpProposeArgs`) | agent's JSON-RPC result verbatim | session-scoped (—) | Async. Sends a JSON-RPC request with `method = action`, `params` forwarded verbatim. |
| 4 | `accept` | `{agent, proposal_id, reason?}` (`AcpDecisionArgs`) | agent's result verbatim | session-scoped (—) | Async. Sends `accept` with `{proposalId, reason?}`. |
| 5 | `reject` | `{agent, proposal_id, reason?}` (`AcpDecisionArgs`) | agent's result verbatim | session-scoped (—) | Async. Sends `reject` with `{proposalId, reason?}`. |
| 6 | `register_server` | `{name, command, args?, capabilities?, disabled?, env?, metadata?, plugin_id}` (`AcpRegisterServerArgs`) | `{ok, status}` where `status` ∈ `ok` / `already_registered` / `invalid_name` / `invalid_command` (`AcpRegisterServerReply`) | `protocol.host.contribute` | Sync. BL-113 contribution registration; invoker-only contribution lifecycle. |
| 7 | `unregister_server` | `{name, plugin_id}` (`AcpUnregisterServerArgs`) | `{ok, status, actual_owner?}` where `status` ∈ `ok` / `not_found` / `not_owned_by_plugin` (`AcpUnregisterServerReply`) | `protocol.host.contribute` | Sync. Plugins can only unregister adapters they contributed (`contributed_by` ownership check). |
| 8 | `disconnect` | `{agent}` (`AcpAgentArgs`) | `{agent, dropped}` | session-scoped (—) | Async. Drops the pool entry and runs graceful shutdown; the reconnect path can re-establish on next call. |

Calling a handler `2/3/4/5/8` through the sync `dispatch` arm returns `PluginError::ExecutionFailed` with reason "requires dispatch_async". The handler-id constants and ordering are registered via `with_v1_aliases(IPC_HANDLERS)` in `nexus-bootstrap/src/plugins/acp.rs`, giving each command both its name and a v1 alias (ADR 0021).

**Inbound server method routing** (separate from the outbound IPC handlers above) — `server::route_method` maps a closed allow-list of inbound JSON-RPC method names to `com.nexus.agent` verbs:

| Inbound JSON-RPC method | Routed IPC call |
|---|---|
| `agent/run` | `com.nexus.agent::session_run` |
| `agent/list` | `com.nexus.agent::session_list` |
| `agent/get` | `com.nexus.agent::session_get` |

Unknown methods → `-32601` (method not found); `ipc_call` failures → `-32000` (server error); `-32602` (`invalid_params_response`) is exposed for callers that pre-validate.

## Capabilities

The crate code performs **no in-process capability checks**. The capability gates are declared in the bootstrap manifest and enforced by the kernel's `ipc_call` boundary, as documented in `docs/0.1.2/ipc-handlers.md`:

- `initialize` → `process.spawn` (spawns an ACP agent child process).
- `register_server` / `unregister_server` → `protocol.host.contribute` (invoker-only contribution lifecycle).
- `list_agents` → none (read-only).
- `propose` / `accept` / `reject` / `disconnect` → none (session-scoped).

The inbound `AcpServer` adds no capability re-checks of its own beyond what the host's `ipc_call` boundary already enforces on `com.nexus.agent`.

## Settings / Config

**`AcpHostConfig`** is the settings type, but it is **not loaded from a TOML file** — there is intentionally no `acp.toml` (ADR 0027 §Phase 4). It is an in-memory registry populated at runtime through `register_server`. The forge `.forge/` layout therefore never owns an ACP config file; an operator wanting a non-plugin adapter ships a minimal manifest-only plugin (as `plugins/first-party-acp-echo/` and the first-party DAP example do).

`AcpHostConfig` fields:
- `adapters: HashMap<String, AcpAdapterSpec>` — keyed by adapter name for O(1) lookup.
- `contributed_by: HashMap<String, String>` — adapter name → contributing plugin's reverse-DNS id; the authorisation key for `unregister_server`. Symmetric with LSP/DAP/MCP shape; every entry in `adapters` also appears here.

`AcpAdapterSpec` fields:
- `name: String` — stable identifier (manifest `id`); routing + authorisation key.
- `command: String` — executable to spawn (looked up on `$PATH` if not absolute).
- `args: Vec<String>` — CLI args appended to `command`.
- `capabilities: Vec<String>` — declarative capability tags advertised by the manifest; surfaced via `list_agents` for an agent picker. **Not gated on at runtime today** — runtime authorisation rides on the kernel capability matrix.
- `disabled: bool` (default `false`) — keep registered but skip spawning; `get_or_connect` errors if disabled.
- `env: HashMap<String, String>` — merged on top of the host process env at spawn (e.g. `ANTHROPIC_API_KEY`).
- `metadata: Option<serde_json::Value>` — opaque shell-only metadata (`plugin_id`, `display_name`, …) packed by `nexus-bootstrap::protocol_host_specs::acp_contribution_to_spec`, stored verbatim, round-tripped through `list_agents`.

Validation (`register_contributed`): empty/whitespace `name` → `InvalidName`; empty/whitespace `command` → `InvalidCommand`; duplicate name → `AlreadyRegistered`. On error the config is unchanged.

**`PoolConfig`** — the only tunable on the pool: `backoff: Vec<Duration>` (default `default_backoff()` = 100ms, 500ms, 2s, 10s, 30s; matches nexus-lsp / nexus-mcp). Retry budget is `1 + backoff.len()` attempts. These are hardcoded, not exposed via TOML.

Other hardcoded timeouts: `DEFAULT_REQUEST_TIMEOUT` = 60s (per outbound request — generous for LLM round trips), `INITIALIZE_TIMEOUT` = 30s (handshake), client shutdown wait = 5s, plugin `on_stop` shutdown deadline = 5s, `DEFAULT_DISPATCH_TIMEOUT` = 600s (inbound server per-`ipc_call`), `ACP_NOTIF_CHANNEL_BOUND` = 1024 (notification channel depth), `MAX_LINE_BYTES` = 16 MiB.

## Events

**Published on the kernel `EventBus` (`publish_plugin`):**
- `com.nexus.acp.started` — emitted from `on_start` with `{registered_agents: <count>}`.
- `com.nexus.acp.<acp_method_with_dots>` — every agent-pushed JSON-RPC notification is republished, with `/` in the method replaced by `.` (e.g. an `agent/output` notification becomes topic `com.nexus.acp.agent.output`), carrying the original JSON-RPC `params` payload verbatim. Republishing happens via `republish_pending`, which drains the client's notification channel after each successful `initialize` / `propose` / `accept` / `reject` proxy call.

**Subscribed:** none. The inbound `AcpServer` ignores client-sent notifications and logs (without acting on) unexpected responses on the inbound stream.

## Internals & notable implementation details

- **JSON-RPC 2.0 framing (`transport.rs`).** `JsonRpcMessage` is an untagged enum decoded by which keys are present. `read_message` reads one line, skips blank/whitespace-only lines (debugger filler), enforces `MAX_LINE_BYTES` before parsing, returns `TransportError::Eof` on a 0-byte read, `Oversized` past the cap, `BadBody` on parse failure. `write_message` serialises to `<json>\n` and flushes — no Content-Length prelude.
- **Outbound client lifecycle (`client.rs`).** `AcpClient::connect` spawns the executable with `current_dir(forge_root)`, all three stdio piped, `kill_on_drop(true)`. A detached task drains the child's stderr into `tracing::debug`. A reader task demultiplexes stdout: `Response` → resolve the matching `oneshot` in the `pending: HashMap<i64, …>` map; `Notification` → `try_send` into a bounded (1024) mpsc channel, dropping with a single latched warn per saturation episode rather than blocking the reader (which also delivers responses); `Request` from the agent → reply `-32601 method not found` (ACP agents don't issue server-initiated requests today, but this prevents a hang). On EOF / transport error the reader drains the pending map with synthetic errors (`-32000` / `-32001`) and exits. `initialize` sends `{processId, clientInfo:{name:"nexus-acp", version}, capabilities}` and stores the response as `server_capabilities`. Request ids are a monotonic `AtomicI64` starting at 1; the stdin writer is behind a `Mutex` so concurrent senders don't tear bytes mid-line.
- **Pool + reconnect (`pool.rs`).** `get_or_connect` returns an existing `Arc<Mutex<AcpClient>>` or lazily connects (errors with a synthetic `Spawn`/`NotFound` for unregistered agents, and a `Spawn`/other for disabled agents). `call_with_reconnect` runs `op` once per attempt; non-transient errors short-circuit, transient errors (`Transport`, `NotRunning`, `RequestTimeout` — classified by `AcpClientError::is_transient`) drop the broken entry, sleep per the backoff schedule, and retry. ACP carries no document-tracking state to resync, so the reconnect loop is a strict subset of LSP's.
- **Core plugin (`core_plugin.rs`).** Holds `config: Arc<RwLock<AcpHostConfig>>` and `pool: Arc<ConnectionPool>`. `on_init` is a no-op (registry intentionally starts empty; contributions arrive after plugin load). `on_start` publishes `com.nexus.acp.started`. `on_stop` spawns a current-thread tokio runtime on a dedicated OS thread to run `pool.shutdown_all()`, hard-capped at a 5s join deadline; on timeout it abandons the join and warns (`audit = true`) that child processes may be stranded. Async handlers snapshot the config (a clone behind `Arc`) before moving into the future, so the `RwLock` isn't held across `await`.
- **Prompt / permission flow.** ACP's `propose` / `accept` / `reject` model the agent-action-approval handshake: `propose` forwards an action as a JSON-RPC method to the agent and returns its result (expected to carry a proposal id); `accept` / `reject` send the decision keyed by `proposalId` with an optional `reason`. The host is a pure forwarder — it does not interpret proposal semantics.
- **Inbound server (`server.rs`).** `serve` reads requests sequentially (preserving wire ordering), routes via the pure `route_method` table, and dispatches through `context.ipc_call`. The outbound writer is wrapped in an `Arc<Mutex<W>>` so responses don't tear. The context is held behind `Arc` because `KernelPluginContext` is not `Clone` (mirrors `nexus_mcp::NexusMcpServer`).

## Tests

The crate's source modules carry inline `#[cfg(test)]` unit tests (there is **no `tests/` directory in the crate**; integration tests live in `nexus-bootstrap`):

- `transport.rs` — round-trips a request, parses responses/notifications, skips blank lines, EOF on empty stream, rejects malformed JSON (and asserts no `Content-Length` in the frame).
- `config.rs` — `register_contributed` happy path / rejects invalid name+command / refuses duplicate; `merge_contributed` preserves input order in the skipped list; `unregister_contributed` round-trip / refuses non-owner / not-found.
- `pool.rs` — pool starts empty; disconnect of unknown returns false; unknown agent errors with `Spawn`.
- `server.rs` — `route_method` covers the three documented verbs + rejects unknowns; `invalid_params_response` shape; `error_response` omits the `result` field.
- `client.rs` — `is_transient` classification (Transport/NotRunning/RequestTimeout transient; Spawn/AgentError/Handshake not).
- `core_plugin.rs` — plugin id correct; `on_init` leaves an empty registry; `list_agents` empty after init; `register_server` round-trip through dispatch (incl. metadata + capabilities round-trip), invalid-name status, duplicate → `already_registered`; `unregister_server` refuses intruder / round-trip; unknown handler id errors; async handlers rejected by sync `dispatch` with a "dispatch_async" message.

**Integration coverage (in `nexus-bootstrap/tests/`):** `acp_server.rs` (happy-path `agent/list` routes through IPC, unknown method → `-32601`, invalid params → server error, pipelined requests preserve response order, graceful disconnect, route table uses the `com.nexus.agent` plugin id) and `acp_contribution_wiring.rs` (BL-113 register/unregister wiring round-trips). A live end-to-end agent process is exercised only through the `first-party-acp-echo` example plugin.

## Gaps / caveats

- **Experimental, no in-tree shell consumer** for the outbound `com.nexus.acp::*` surface as of 0.1.2 (per `core_plugin.rs` and `docs/0.1.2/plugins/assessment/PHASE5_DECISIONS.md` §5.1). Only the inbound `nexus acp serve` path and the echo example use the crate.
- **`list_agents` always reports `connected: false`** — the sync handler can't await the pool's connected set; a true status column would require an async list variant.
- **`AcpConfigError`** exists only to keep the public surface symmetric with the other protocol-host crates; there is no flat-TOML loader today, so it is effectively dead code awaiting a hypothetical future escape hatch.
- **`capabilities` on an adapter spec is advertising-only** — surfaced through `list_agents` but not enforced by the host at runtime.
