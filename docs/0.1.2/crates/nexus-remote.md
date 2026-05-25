# nexus-remote

> Kind: lib · IPC plugin id: — · CorePlugin: no · Has settings: no (compile-time constants only) · As of: 2026-05-25

## Overview

`nexus-remote` is the **remote-forge JSON-RPC server** (BL-140). It exposes a headless Nexus kernel's full IPC surface and event bus over a line-delimited JSON-RPC 2.0 stdio stream, so a frontend (CLI, TUI, Tauri shell) can drive a Nexus instance running somewhere else — in Phase 1, a child process; in Phase 2, an `ssh user@host nexus serve --stdio` subprocess. The crate ships both halves of the wire contract: a [`RemoteServer`] (the headless side) and a [`RemoteClient`] (the embedding/frontend side), plus the framing codec they share and a parser for `ssh://` forge URIs.

The server is a **pure proxy**, not a service plugin. It owns no state of its own beyond a per-connection subscription registry. Every `ipc_call` request is routed verbatim through `KernelPluginContext::ipc_call(plugin_id, command, args, timeout)` — the same boundary the CLI, TUI, MCP server, and Tauri shell use. There is deliberately **no allow-list**: the point of remote forge is full IPC-surface access, with trust delegated to the transport layer (SSH in Phase 2) rather than to a per-method gate. `event_subscribe` opens a kernel `EventBus` subscription and forwards matching events back as server-pushed `event` notifications.

In microkernel terms, `nexus-remote` is a **transport adapter** sitting at the same architectural layer as `nexus-mcp` or `nexus-acp`: it is a frontend onto the kernel, consumed by `nexus-cli`, never registered as a `CorePlugin` and never depended on by the kernel. The CLI's `nexus serve --stdio` subcommand builds a normal CLI runtime via `nexus-bootstrap`, then hands the kernel + plugin context to a `RemoteServer` and blocks on `tokio::io::{stdin, stdout}` until the parent disconnects.

The framing and JSON-RPC envelope types are intentionally **duplicated from `nexus-acp::transport`** rather than shared. Per the BL-140 design call, the two crates should evolve their method surfaces independently — ACP's narrow allow-list shouldn't constrain a remote-forge proxy that exposes the whole IPC tree.

## Position in the dependency graph

- **Direct `nexus-*` deps:** `nexus-kernel` (`KernelPluginContext`, `EventBus`, the `Ipc` trait providing `ipc_call`), `nexus-plugin-api` (`EventFilter`, `PublishedEvent`, `EventMetadata`, `NexusEvent`).
- **Notable external deps:** `tokio` (async runtime, `AsyncRead`/`AsyncWrite`, `Mutex`, `Notify`, `JoinSet`, `mpsc`/`oneshot`), `serde` / `serde_json` (wire envelopes), `thiserror`, `tracing`, `uuid`.
- **Dev-deps:** `nexus-bootstrap` (boots a real CLI runtime for the integration tests), `tempfile`, `chrono`, `tokio` with `test-util` + `io-util`.
- **Crates depending on it:** `nexus-cli` only. The CLI uses `RemoteServer` (in `commands/serve.rs`) and `ForgeUri` (in `main.rs` / `app.rs`, to detect `ssh://` forge paths).

## Public API surface

Re-exported from `lib.rs`:

| Module | Item | Purpose |
|---|---|---|
| `server` | `RemoteServer` | The headless JSON-RPC server. `new(context, event_bus)` → builder; `with_timeout(d)` overrides the default dispatch timeout; `serve(reader, writer)` runs the read/dispatch/write loop until EOF. |
| `server` | `RemoteServerError` | `Transport` (inbound wire failure) / `Write` (outbound pipe broke). |
| `server` | `DEFAULT_DISPATCH_TIMEOUT` (600s), `MAX_DISPATCH_TIMEOUT` (3600s) | Per-`ipc_call` default and the hard ceiling on a client-requested override. (Not re-exported at crate root; module-public.) |
| `client` | `RemoteClient` | The frontend side. `new(reader, boxed_writer)` spawns a response-router task; `ipc_call` / `subscribe` / `unsubscribe` / `shutdown` / `wait_for_disconnect` / `with_default_timeout`. |
| `client` | `RemoteClientError` | `Transport` / `RouterStopped` / `Server{code,message}` / `Timeout(d)` / `MalformedResponse`. |
| `client` | `EventDelivery` | An inbound `event` notification: `{ subscription_id, event: Value }`. |
| `client` | `DEFAULT_CALL_TIMEOUT` (600s) | Mirrors the server default. |
| `transport` | `JsonRpcMessage` (untagged enum), `JsonRpcRequest`, `JsonRpcResponse`, `JsonRpcError`, `JsonRpcNotification` | JSON-RPC 2.0 envelope types. |
| `transport` | `read_message` / `write_message` | Line-delimited framing read/write over any `AsyncRead`/`AsyncWrite`. |
| `transport` | `TransportError`, `MAX_LINE_BYTES` (16 MiB) | Framing errors and the per-line ceiling. |
| `uri` | `ForgeUri` (enum, `Ssh` variant), `SshForgeUri`, `ParseError` (re-exported as `ForgeUriParseError`) | Parser + `Display` for `ssh://[user@]host[:port]/abs/path` forge URIs, including bracketed IPv6 literals. |

## JSON-RPC method surface

The server recognises exactly three client-callable methods. All routing happens in `RemoteServer::dispatch_request`; each spawns a task into a per-connection `JoinSet` so a slow call doesn't head-of-line-block the connection.

| Method | Params | Result | Maps to | Description |
|---|---|---|---|---|
| `ipc_call` | `{ plugin_id: string, command: string, args?: any, timeout_ms?: u64 }` | The raw `serde_json::Value` returned by the target plugin | `KernelPluginContext::ipc_call(plugin_id, command, args, timeout)` | Routed verbatim, no allow-list. `args` defaults to `null`; `timeout_ms` is optional, must be `> 0`, and is capped at `MAX_DISPATCH_TIMEOUT` (3600s) — absent/null uses the server default (600s). |
| `event_subscribe` | `{ subscription_id: string, filter: {kind, …} }` | `{ subscription_id }` | `EventBus::subscribe(filter)` | Registers a long-lived forwarder task. Matching events stream back as `event` notifications carrying the supplied `subscription_id`. Duplicate ids are rejected with `-32000`. |
| `event_unsubscribe` | `{ subscription_id: string }` | `{ ok: true }` or `{ ok: false, reason: "unknown subscription_id" }` | aborts the forwarder `JoinHandle` | Cancels one subscription. Unknown ids report `ok:false` rather than erroring, so callers can unsubscribe twice safely. |

**Server-pushed notification** (not a request — no `id`):

| Method | Params | Description |
|---|---|---|
| `event` | `{ subscription_id: string, event: <serialised PublishedEvent> }` | One delivered bus event. The `event` payload is a serde-serialised `PublishedEvent` — the same shape the shell already consumes from `kernel_subscribe` over Tauri IPC, so remote clients can reuse existing decoders. |

**Filter wire shapes** (the `filter` field of `event_subscribe`), parsed into `EventFilter`:
- `{ "kind": "all" }`
- `{ "kind": "variant", "name": "PluginLoaded" }`
- `{ "kind": "custom_prefix", "prefix": "com.nexus.editor." }`
- `{ "kind": "custom_exact", "type_id": "com.nexus.editor.saved" }`

**Error codes:** unknown method → `-32601`; invalid/missing params → `-32602`; underlying `ipc_call` failure or duplicate-subscription → `-32000`. Client-pushed notifications and stray responses on the inbound stream are silently ignored / logged (the server never accepts client notifications today).

## IPC handlers

**None.** `nexus-remote` registers no IPC handler and is not a `CorePlugin` — it is a frontend/transport adapter, the inverse role of a service crate. It *consumes* the IPC boundary (`ipc_call`) rather than contributing to it. New backend capability still belongs in the relevant `nexus-<service>` crate; this crate transparently re-exports whatever the kernel already routes.

## Capabilities

The server performs **no capability checks of its own**. Capability gating is fully delegated to the kernel: every proxied `ipc_call` flows through `KernelPluginContext::ipc_call`, which enforces the same capability gates (`fs.read`, `kv.write`, `ipc.call`, etc.) it would for any local caller. There is no per-method allow-list at the remote boundary — the design treats the transport (SSH auth in Phase 2) as the trust gate, on the premise that anyone who can open the stdio stream is already trusted to drive the forge. Event subscriptions go through `EventBus::subscribe`, subject to whatever the bus enforces.

## Settings / Config

No runtime config file and no `Config` struct. Behaviour is governed by compile-time constants:

- `DEFAULT_DISPATCH_TIMEOUT` = 600s (server-side default per `ipc_call`).
- `MAX_DISPATCH_TIMEOUT` = 3600s (hard ceiling on a client-requested `timeout_ms`; over-large requests are silently clamped, not rejected).
- `MAX_LINE_BYTES` = 16 MiB (per-line framing ceiling; oversized lines close the connection).
- `DEFAULT_CALL_TIMEOUT` = 600s (client-side default).

The server's default timeout is overridable per-instance via `RemoteServer::with_timeout` (used by tests); the client's via `RemoteClient::with_default_timeout`. There is no `.forge/` TOML for this crate.

## Events

Yes — event streaming to remote clients is a first-class feature. On `event_subscribe`, the server spawns a forwarder task that loops on `bus.subscribe(filter).recv()` and writes each matching `PublishedEvent` as an `event` JSON-RPC notification carrying the subscription id. The forwarder stops when the client sends `event_unsubscribe` (the `JoinHandle` is aborted), when the outbound pipe breaks, or when `serve` returns (all subscription tasks are aborted via `abort_all`). On the client side, the router task fans `event` notifications out to per-subscription `mpsc::UnboundedSender<EventDelivery>` sinks keyed by `subscription_id`.

## Internals & notable implementation details

- **Framing (`transport.rs`):** newline-delimited JSON-RPC 2.0 — one JSON object per line, `\n`-terminated, **no `Content-Length` header**. `read_message` skips blank/whitespace-only lines, returns `TransportError::Eof` on a clean close, `Oversized` past `MAX_LINE_BYTES`, and `BadBody` on malformed JSON (never silently dropped). `JsonRpcMessage` is `#[serde(untagged)]`, so request/response/notification are disambiguated structurally. Responses skip-serialise `None` `result`/`error` fields to stay JSON-RPC-compliant.
- **Server concurrency (`server.rs`):** the `serve` loop reads sequentially but dispatches each request into a connection-scoped `tokio::task::JoinSet` (`pending`), reaped non-blockingly via `try_join_next` each iteration so it can't grow unbounded under a hot client. This is the **D1 audit fix (2026-05-21)**: previously handlers were fire-and-forget, so dropping the serve future abandoned in-flight `ipc_call`s mid-write; the `JoinSet` now aborts outstanding work on drop. On EOF, the loop aborts all subscriptions, then gives in-flight handlers a 2s grace window (mirroring the kernel bootstrap shutdown patience) before the `JoinSet` drop aborts them. The single outbound writer is shared behind an `Arc<Mutex<W>>` and locked only briefly per write.
- **Headless bootstrap:** `nexus serve --stdio` (CLI `commands/serve.rs`) builds a standard runtime with `build_cli_runtime(forge_root)`, destructures `Runtime { kernel, context, loader }`, grabs `kernel.event_bus()`, constructs `RemoteServer::new(Arc::new(context), event_bus)`, then runs a multi-thread tokio runtime (`max_blocking_threads = KERNEL_BLOCKING_POOL_SIZE`) blocking on `server.serve(stdin(), stdout())`. The kernel is held alive for the loop's duration and dropped at scope exit for clean plugin shutdown. The `ServeArgs` struct exposes one flag, `--stdio` (required in Phase 1; `--port`/`--unix-socket` are noted as future transports).
- **Client router (`client.rs`):** `RemoteClient::new` spawns a background `run_router` task that demultiplexes inbound frames — responses resolve a per-request `oneshot` keyed by integer id; `event` notifications fan out to subscriber `mpsc` channels. Request ids are a monotonic `AtomicU64`. `subscribe` pre-registers its sink *before* sending the request (so a fast event can't miss its channel) and rolls it back on failure. When the router exits it clears `pending` (waking blocked callers with `RouterStopped`), fires a `Notify` (`wait_for_disconnect`, for a BL-146 watchdog above the client), and `Drop` aborts the router via `try_lock`.
- **URI parsing (`uri.rs`):** hand-rolled parser for `ssh://[user@]host[:port]/abs/path` with explicit error variants (`NoScheme`, `UnsupportedScheme`, `EmptyAuthority`/`User`/`Host`, `InvalidPort`, `MissingPath`, `RelativePath`, `MalformedBracketedHost`). Handles bracketed IPv6 literals (`[::1]:22`), requires an absolute path, and round-trips via `Display` (re-bracketing IPv6 hosts on output). Phase 2b wiring (the factory that turns a `ForgeUri::Ssh` into a real SSH child process) lives in the CLI, not here.
- **Error mapping:** server maps `ipc_call` failures to `-32000`, unknown methods to `-32601`, bad params to `-32602`; the client surfaces JSON-RPC error envelopes as `RemoteClientError::Server { code, message }`.

## Tests

- **Unit tests in `server.rs`** (16): `parse_filter` for all four filter kinds + rejection cases; `parse_timeout_ms` (default/null/positive/cap/zero/non-numeric); `error_response` omits the `result` field; `invalid_params` shape; `build_event_notification` carries id + event object.
- **Unit tests in `transport.rs`** (6): request round-trip (asserts no `Content-Length`), response parse, notification parse, blank-line skipping, EOF on empty stream, malformed-JSON rejection.
- **Unit tests in `client.rs`** (3): `result_or_err` surfaces error envelopes, returns values, rejects empty responses.
- **Unit tests in `uri.rs`** (~25): full parse matrix (user/host/port/path permutations, IPv4 + bracketed IPv6 literals, paths with spaces) plus every rejection variant and three `Display` round-trips.
- **`tests/end_to_end.rs`** (8): boots a real CLI runtime over `tokio::io::duplex` and drives the *server* directly — `ipc_call` round-trip to `com.nexus.storage::list_dir`, unknown plugin → `-32000`, unknown method → `-32601`, missing params → `-32602`, subscribe→publish→`event` notification→unsubscribe, missing filter → `-32602`, unknown-unsubscribe `ok:false`, duplicate-id rejection. The kernel is `Box::leak`ed for the test's lifetime (dropping it mid-test tears down the IPC surface).
- **`tests/client_server_loop.rs`** (8): the Phase 2a integration test — wires a `RemoteClient` to an in-process `RemoteServer` over duplex and exercises every public client method (`ipc_call` success/error/per-call-timeout, subscribe/unsubscribe round-trip, invalid filter, unknown-unsubscribe, duplicate id, and `shutdown` waking pending calls). Deliberately does **not** spawn an SSH child process, to keep CI hermetic.

## Gaps / notes

- **SSH transport (Phase 2b) is not in this crate.** `uri.rs` only parses `ssh://` URIs; the factory that spawns the actual `ssh … nexus serve --stdio` child and builds a `RemoteClient` around its stdio lives in `nexus-cli` (`app.rs`'s `App::new_remote`, reached from `main.rs` when the forge path contains `://`). This doc covers the wire layer; the SSH process-spawning path is out of scope here and worth documenting in the `nexus-cli` crate doc.
- The crate has no binary target; `nexus serve` is the only entry point, owned by `nexus-cli`.
