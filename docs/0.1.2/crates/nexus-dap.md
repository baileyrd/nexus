# nexus-dap

> Kind: lib · IPC plugin id: com.nexus.dap · CorePlugin: yes · Has settings: DapHostConfig · As of: 2026-05-25

## Overview

`nexus-dap` is the Debug Adapter Protocol (DAP) host. It spawns external
debug adapters (codelldb, debugpy, js-debug, …) as child processes,
frames DAP messages over their stdio with the same Content-Length
envelope LSP uses, and bridges the protocol to every forge client
through one IPC surface (`com.nexus.dap`). A frontend never speaks DAP
directly — it issues high-level verbs (`launch`, `set_breakpoints`,
`continue`, `variables`, …) and the host translates them into DAP
requests, correlates responses, and republishes adapter-pushed
notifications on the kernel event bus as `com.nexus.dap.<event>`.

The crate is, by deliberate construction, a near-1:1 mirror of
[`nexus-lsp`](nexus-lsp.md): the same `transport` Content-Length codec,
the same per-adapter `ConnectionPool` with lazy connect and
reconnect-with-backoff, the same `tracing`/`thiserror` posture, and the
same `ts-export` feature gate for binding/schema generation. The
protocol-specific differences are contained in `protocol`: DAP's
`type`-tagged envelope (`request` / `response` / `event`), the
request/response/event triplet, and the per-direction `seq` correlation
id in place of JSON-RPC's `id`.

Two state machines sit above the codec. `DapClient` owns one running
adapter child: it drives the `initialize` handshake, allocates outbound
`seq` numbers, demultiplexes inbound messages into oneshot pending-request
slots vs. a bounded event channel, caches the breakpoint set per source
for resync, and shuts the child down gracefully. `ConnectionPool` keys
clients by adapter name, connects lazily on first use, and wraps every
operation in `call_with_reconnect` so a transient adapter crash triggers
a reconnect, a breakpoint replay, and a retry against the backoff
schedule.

Microkernel fit: `nexus-dap` is a service plugin. It depends on the
kernel (for the event bus) and `nexus-plugins` (for the `CorePlugin`
trait), never the reverse. All capability it offers reaches CLI, TUI,
MCP, and the shell uniformly through `context.ipc_call("com.nexus.dap",
…)`. Adapters can be configured statically in `dap.toml` or contributed
at runtime by other plugins via the BL-113 / ADR 0027
`register_adapter` / `unregister_adapter` verbs, with TOML entries
always winning a name collision.

## Position in the dependency graph

- **Direct `nexus-*` deps:** `nexus-kernel` (`EventBus` for republishing
  adapter events), `nexus-plugins` (`CorePlugin`, `CorePluginFuture`,
  `PluginError`).
- **Notable external deps:** `tokio` (process spawn, async I/O, channels,
  timeouts), `serde` / `serde_json` (wire envelope + IPC payloads),
  `toml` (parse `dap.toml`), `thiserror` (typed errors), `schemars`
  (JSON Schema for IPC types), `tracing`; `ts-rs` is optional behind the
  `ts-export` feature (TS bindings + schema export, kept out of
  production builds, mirroring `nexus-lsp` / `nexus-mcp`).
- **Crates depending on it:** `nexus-bootstrap` — registers
  `DapCorePlugin` in `src/plugins/dap.rs` and drives runtime contribution
  wiring in `src/dap_contribution_wiring.rs` (which calls
  `register_adapter` / `unregister_adapter` per manifest-declared adapter
  via `protocol_host_specs::dap_contributions_to_specs`). No other crate
  links it directly; frontends reach it only over IPC.

## Public API surface

Re-exported from `lib.rs`:

- **`config`** — `dap.toml` parser and the runtime adapter registry.
  `DapHostConfig` (the settings struct: `adapters` map +
  `contributed_by` provenance map), `DapAdapterSpec` (one adapter
  definition), `DapConfigError`, plus the contribution-merge result
  types `MergeSkip` / `MergeSkipReason` (re-exported as
  `DapMergeSkip` / `DapMergeSkipReason`) and `UnregisterError`.
- **`protocol`** — the DAP wire envelope. `ProtocolMessage` (a
  `#[serde(tag = "type")]` enum of `Request` / `Response` / `Event`),
  `ProtocolRequest`, `ProtocolResponse`, `ProtocolEvent`. One-line role:
  serde-defined types that round-trip the exact on-wire JSON.
- **`transport`** — wire-layer Content-Length framing only.
  `read_message` / `write_message` over any async reader/writer,
  `TransportError`. 16 MiB body cap; `Content-Type` accepted and ignored.
- **`client`** — `DapClient` (one adapter child + its lifecycle),
  `DapClientError` (with `is_transient()` for the pool), `AdapterEvent`
  (an adapter notification queued for republish), `AdapterCapabilities`
  (typed booleans from the `initialize` reply + the raw payload),
  `SourceBreakpointSpec` (one cached breakpoint).
- **`pool`** — `ConnectionPool` (adapter-name-keyed client pool with
  lazy connect / reconnect / breakpoint replay / shutdown-all) and
  `PoolConfig` (`backoff` schedule; `default_backoff()` is
  100ms/500ms/2s/10s/30s, matching `nexus-lsp`).
- **`core_plugin`** — `DapCorePlugin` (the `CorePlugin` impl), plus the
  `PLUGIN_ID` constant, the per-verb `HANDLER_*` ids, the
  `IPC_HANDLERS` `(name, id)` table consumed by bootstrap (SD-06), and
  the `ASYNC_HANDLERS` set.
- **`ipc`** *(module exists but is not re-exported from `lib.rs`)* —
  wire-mirror `serde` + `schemars` (+ optional `ts-rs`) structs for
  every command's args/reply (`DapLaunchArgs`, `DapSetBreakpointsArgs`,
  `DapThreadArgs`, `DapRegisterAdapterArgs`, `DapRegisterAdapterReply`,
  …). These exist purely so the schema generator and the shell have a
  concrete contract; the handlers themselves build replies with ad-hoc
  `json!` macros (same pattern as `nexus-lsp::ipc` / `nexus-mcp::ipc`).

## IPC handlers

21 handlers (`IPC_HANDLERS`, ids 1..=21, contiguous). All session /
execution / inspection verbs (ids 2..=19) are async (`dispatch_async`);
`list_adapters` (1) has both a sync arm — configured set only,
`connected: false` for every row — and an async arm that adds live
`connected` state from the pool. `register_adapter` (20) /
`unregister_adapter` (21) are sync (they only take a write lock on the
in-memory map). The "Capability" column reflects the intended/documented
gate in `ipc-handlers.md`; see [Capabilities](#capabilities) for the
important caveat that the crate itself does **not** enforce these at the
verb level today.

| Command | Args | Returns | Capability | Description |
|---------|------|---------|------------|-------------|
| `list_adapters` | `{}` | array of `DapAdapterEntry` (`name`, `command`, `args`, `adapter_type`, `file_types`, `disabled`, `connected`, `metadata`) | — | Configured adapters; async arm marks live `connected` from the pool. |
| `launch` | `DapLaunchArgs` (`adapter`, `program`, opt `mode`, `args`, `cwd`, `env`, `stop_on_entry`, `extra`) | DAP `launch` response `body` (or `null`) | `process.spawn` | Lazily spawn the adapter, send `launch`. `extra` keys merged (existing keys win). |
| `attach` | `DapAttachArgs` (`adapter`, opt `pid`, `port`, `extra`) | DAP `attach` response `body` (or `null`) | `process.spawn` | Spawn adapter, send `attach` for PID/port targets. |
| `configuration_done` | `DapAdapterArgs` (`adapter`) | `{ "ok": true }` | — | Post-breakpoint handshake (`configurationDone`). |
| `disconnect` | `DapAdapterArgs` (`adapter`, opt `terminate_debuggee`) | `{ "ok": true }` | — | Graceful tear-down; sets DAP `restart:false`, `terminateDebuggee`. |
| `terminate` | `DapAdapterArgs` (`adapter`) | `{ "ok": true }` | — | Force-stop the debuggee (`terminate`). |
| `set_breakpoints` | `DapSetBreakpointsArgs` (`adapter`, `source_path`, `breakpoints[]` of `{line, condition?, hit_condition?, log_message?}`) | DAP `setBreakpoints` response `body` (or `null`) | — | Replace per-source breakpoints; caches them on the client for resync. |
| `set_function_breakpoints` | `DapSetFunctionBreakpointsArgs` (`adapter`, `breakpoints[]` of `{name, condition?}`) | DAP `setFunctionBreakpoints` response `body` (or `null`) | — | Function-name breakpoints. |
| `set_exception_breakpoints` | `DapSetExceptionBreakpointsArgs` (`adapter`, `filters[]`) | `{ "ok": true }` | — | Exception filters (e.g. `raised` / `uncaught`). |
| `continue` | `DapThreadArgs` (`adapter`, `thread_id`) | `{ "ok": true }` | — | Resume. |
| `next` | `DapThreadArgs` | `{ "ok": true }` | — | Step over. |
| `step_in` | `DapThreadArgs` | `{ "ok": true }` | — | Step in. |
| `step_out` | `DapThreadArgs` | `{ "ok": true }` | — | Step out. |
| `pause` | `DapThreadArgs` | `{ "ok": true }` | — | Request a stop. |
| `threads` | `DapAdapterArgs` (`adapter`) | DAP `threads` response `body` | — | Enumerate threads. |
| `stack_trace` | `DapStackTraceArgs` (`adapter`, `thread_id`, opt `start_frame`, `levels`) | DAP `stackTrace` response `body` | — | Frames for a thread. |
| `scopes` | `DapScopesArgs` (`adapter`, `frame_id`) | DAP `scopes` response `body` | — | Scopes for a frame. |
| `variables` | `DapVariablesArgs` (`adapter`, `variables_reference`, opt `filter`, `start`, `count`) | DAP `variables` response `body` | — | Resolve a `variablesReference`. |
| `evaluate` | `DapEvaluateArgs` (`adapter`, `expression`, opt `frame_id`, `context`) | DAP `evaluate` response `body` | — | REPL / watch / hover evaluation. |
| `register_adapter` | `DapRegisterAdapterArgs` (`name`, `command`, `args`, `adapter_type?`, `file_types`, `disabled`, `env`, `plugin_id`, `metadata?`) | `DapRegisterAdapterReply` `{ ok, status }` — `status` ∈ `ok` / `toml_override` / `invalid_name` / `invalid_command` | `protocol.host.contribute` (documented; not verb-enforced) | BL-113 runtime add of a plugin-contributed adapter. |
| `unregister_adapter` | `DapUnregisterAdapterArgs` (`name`, `plugin_id`) | `DapUnregisterAdapterReply` `{ ok, status, actual_owner? }` — `status` ∈ `ok` / `not_found` / `toml_entry` / `not_owned_by_plugin` | `protocol.host.contribute` (documented; not verb-enforced) | BL-113 runtime remove; `plugin_id` must match the contributing owner. |

Error / arg-validation behaviour:

- A missing/ill-typed required arg in `dispatch_async` makes the handler
  return `None` (no future), which the kernel surfaces as a dispatch
  failure rather than a malformed adapter call.
- Adapter/transport failures map through `map_client_err` to
  `PluginError::ExecutionFailed`.
- `register_adapter` / `unregister_adapter` surface validation outcomes
  as a `{ ok:false, status:… }` envelope (not an error) so the caller
  can decide whether to log-and-continue; only a missing/empty `name`,
  `command`, or `plugin_id` raises `PluginError::ExecutionFailed`.

## Capabilities

The intended/documented gates (per `docs/0.1.2/ipc-handlers.md`) are
`process.spawn` for `launch` / `attach` and `protocol.host.contribute`
for `register_adapter` / `unregister_adapter`; the remaining verbs are
ungated session/inspection control.

**Important caveat, grounded in source:** `nexus-dap` itself performs
**no capability check at the IPC-verb level**. `DapClient::connect`
spawns the adapter child unconditionally via
`tokio::process::Command`, and the `register_adapter` doc-comment in
`core_plugin.rs` states the trust model explicitly — there is no
verb-level capability gate; hard enforcement needs kernel-side
caller-identity threading and is filed as a hardening follow-up. Any
capability enforcement therefore lives in the kernel's IPC dispatcher
/ manifest layer, not in this crate. The crate declares no capabilities
of its own; `nexus-bootstrap`'s `register` does not grant any either.

## Settings / Config

`DapHostConfig` is parsed from `<forge>/.forge/dap.toml` at `on_init`.
A missing file yields an empty config so a forge with no adapters boots
cleanly; a parse error is logged and also degrades to empty ("DAP host
disabled") rather than failing startup.

`dap.toml` schema (array-of-tables keyed by `name`, same shape as
`lsp.toml`):

```toml
[[adapters]]
name = "rust"                     # required, unique; the `adapter` IPC arg
command = "codelldb"              # required; looked up on $PATH if not absolute
args = ["--port", "0"]            # optional
type = "lldb"                     # optional cosmetic hint (→ adapter_type)
file_types = ["rs", "c", "cpp"]   # optional; used by adapter_for_path
disabled = false                  # optional; keep entry but skip spawning

[adapters.env]                    # optional; merged on top of host env
RUST_BACKTRACE = "1"
```

`DapAdapterSpec` fields: `name`, `command`, `args` (default `[]`),
`adapter_type` (TOML key `type`, optional), `file_types` (default `[]`),
`disabled` (default `false`), `env` (default `{}`), and `metadata`
(default `None`). `metadata` is the BL-113 opaque shell-facing payload:
TOML entries always have `metadata = None` (TOML uses
`deny_unknown_fields`, so there's no TOML key for it); plugin
contributions populate it via `nexus-bootstrap::dap_contribution_to_spec`
with `{"launch_config_schema": <inline JSON Schema>, …}` so the shell can
render a typed launch-config form. The host never interprets it; it
round-trips verbatim through `list_adapters`.

`DapHostConfig` carries two maps: `adapters` (name → spec, O(1) lookup)
and `contributed_by` (name → contributing plugin reverse-DNS id). The
runtime registry mutators are `merge_contributed` (batch),
`register_contributed` (single; the `register_adapter` entry point), and
`unregister_contributed` (the `unregister_adapter` entry point).
Precedence is **TOML wins**: a contributed name that collides with any
existing entry (TOML or another plugin's) is rejected as
`TomlOverride`; only `contributed_by`-recorded entries can be
unregistered, and only by their recording plugin. `adapter_for_path`
selects the first enabled adapter whose `file_types` contains the path's
extension (case-insensitive).

`PoolConfig`/`default_backoff` (100ms, 500ms, 2s, 10s, 30s) and the
client timeouts (`DEFAULT_REQUEST_TIMEOUT` 10s, `INITIALIZE_TIMEOUT`
20s, 2s graceful-disconnect, 5s child-wait-then-kill, 5s on-stop
shutdown deadline) and the `DAP_EVENT_CHANNEL_BOUND` (1024) /
`MAX_BODY_BYTES` (16 MiB) are hardcoded constants, not TOML-configurable.

## Events

**Published** on the kernel event bus (via `EventBus::publish_plugin`):

- `com.nexus.dap.started` — emitted in `on_start` with
  `{ "configured_adapters": <count> }`.
- `com.nexus.dap.<event>` — every adapter-pushed DAP event is fanned out
  with the adapter `body` preserved verbatim. Known DAP events:
  `initialized`, `stopped`, `continued`, `exited`, `terminated`,
  `thread`, `output`, `breakpoint`, `module`, `process`, `capabilities`.
  Unknown event names pass through unchanged. Republishing happens via
  `republish_pending`, which drains the client's event queue on each
  successful command attempt (there is no standalone background poll
  loop — events surface piggybacked on the next IPC verb).

**Subscribed:** none. The crate does not consume kernel events.

## Internals & notable implementation details

- **Adapter spawn + lifecycle (`client.rs`).** `DapClient::connect`
  spawns `command` + `args` with `spec.env` merged on top of the host
  environment, all three stdio pipes piped, and `kill_on_drop(true)`. A
  dedicated task drains the child's stderr to `tracing::debug` so the
  pipe never blocks. A reader task owns stdout. On drop without an
  explicit `shutdown`, a debug line is logged; `shutdown` sends
  `disconnect` (2s budget), shuts stdin, then waits up to 5s for exit
  before killing.
- **Content-Length framing (`transport.rs`).** Identical to LSP:
  CRLF-terminated headers, blank line, then exactly `Content-Length`
  body bytes. `Content-Type` is accepted and ignored; a 16 MiB body cap
  guards against runaway `variables` replies. `read_message` returns
  `TransportError::Eof` only when the stream closes before any header
  byte (the canonical "child exited" path), distinct from
  `BadHeader("stream closed mid-header")`.
- **Initialize / launch / attach sequence.** `connect` runs
  `initialize` (20s timeout) with a fixed client capability set
  (`linesStartAt1`, `columnsStartAt1`, `pathFormat:"path"`,
  `supportsRunInTerminalRequest:false`, …); `adapterID` is
  `adapter_type` if set, else the adapter name. The reply's capability
  booleans are parsed into `AdapterCapabilities` (five typed flags +
  full `raw`). Callers then drive `launch`/`attach`, `setBreakpoints`,
  `configurationDone`, and execution-control verbs.
- **Request/response correlation.** Outbound requests get a monotonic
  `seq` from an `AtomicI64`; a oneshot sender is parked in a `pending`
  map keyed by `seq`. The reader task routes `Response` by
  `request_seq` into the matching oneshot, drops the slot, and warns on
  an unknown `request_seq`. `send_request` waits with a 10s timeout and
  classifies the outcome (success → `body`; `success:false` →
  `AdapterError`; empty-command synthetic response or closed channel →
  `NotRunning`; timeout → `RequestTimeout`).
- **Event delivery + backpressure.** Adapter `Event` messages go onto a
  bounded (1024) mpsc channel via `try_send`; when the consumer falls
  behind, excess events are dropped and a single latched warn is logged
  per saturation episode (resets on the next successful send). The host
  drains via `drain_events`; tests use `next_event`.
- **Adapter-initiated requests.** DAP allows server→client requests
  (`runInTerminal`, `startDebugging`). The reader replies
  `success:false` with an explanatory message (using a separate reply-seq
  counter so it never collides with caller-tracked ids) so the adapter
  falls back instead of hanging.
- **Reconnect + breakpoint replay (`pool.rs`).** `call_with_reconnect`
  runs an op closure with `1 + backoff.len()` attempts. A non-transient
  error returns immediately; a transient one snapshots the broken
  client's cached breakpoints, evicts the pool entry, sleeps the backoff
  step, reconnects, and replays the cached breakpoints (best-effort —
  replay failures are logged, not fatal) before retrying. Breakpoints
  are remembered per source via `remember_breakpoints` alongside each
  `set_breakpoints` call.
- **EOF handling.** When the reader hits EOF or a transport error, it
  drains the pending map and synthesises failure responses (empty
  `command`) so blocked callers unblock as `NotRunning`, which the pool
  treats as transient and reconnects.
- **Config mutation under load.** The active `DapHostConfig` lives behind
  an `Arc<RwLock<…>>`. Async handlers take an immutable per-future
  snapshot at dispatch time (`snapshot_config`), so an in-flight command
  keeps the adapter view it started with even if a concurrent
  `register_adapter` / `unregister_adapter` mutates the master config.
- **on_stop.** Runs `pool.shutdown_all()` on a throwaway current-thread
  Tokio runtime inside a spawned OS thread, polled against a 5s deadline;
  on timeout it logs an `audit = true` warning and abandons the join,
  noting child processes may be stranded until the host process exits.
- **Contribution model.** Adapters arrive either from `dap.toml`
  (provenance-free, TOML-pinned) or as plugin contributions. Bootstrap's
  `dap_contribution_wiring` iterates manifest-declared DAP contributions
  and dispatches one `register_adapter` IPC call per `(spec, plugin_id)`
  pair (5s per-call timeout), with the symmetric
  `unwire_dap_contributions_for_plugin` for disable/shutdown. Per-adapter
  failures are logged and don't abort the wire pass. The host stays
  protocol-only (ADR 0027): shell-only cosmetic fields ride in the opaque
  `metadata` payload, never interpreted by the host.

## Tests

- **`src/config.rs` (unit).** `dap.toml` parsing (missing file → empty,
  two-block parse, duplicate-name error, empty-command rejection),
  `adapter_for_path` (case-insensitive extension match, skips disabled),
  and the BL-113 merge/register/unregister rules
  (`merge_contributed` insert / TOML-wins collision / empty
  name+command rejection / input-order preservation /
  `contributed_by` population; `register_contributed` happy path +
  collisions; `unregister_contributed` owner-match removal and the
  `NotFound` / `TomlEntry` / `NotOwnedByPlugin` distinction).
- **`src/protocol.rs` (unit).** serde round-trips for request /
  response (incl. error message, `body:None` omitted) / event, and
  rejection of an unknown `type` discriminator.
- **`src/transport.rs` (unit, tokio).** Request/response/event
  round-trips, `Content-Type` ignored, EOF-before-any-byte →
  `Eof`, missing `Content-Length` → `BadHeader`, oversized message
  rejected, malformed body → `BadBody`.
- **`src/client.rs` (unit).** `AdapterCapabilities` parsing (known
  flags / empty body → defaults) and `DapClientError::is_transient`
  classification.
- **`src/pool.rs` (unit, tokio).** Pool starts empty, disconnect of
  unknown returns false, unconfigured/disabled adapter → `Spawn` error,
  `spec_to_wire` field omission/inclusion.
- **`src/core_plugin.rs` (unit).** `PLUGIN_ID` constant, handler ids
  unique+contiguous 1..=21, `on_init` with missing/valid/invalid
  `dap.toml`, sync `list_adapters` shape, unknown handler error,
  async handler returns `None` on missing required args,
  `on_start`/`on_stop` safety, `parse_source_breakpoint` +
  `spec_to_wire` camelCase, the `ASYNC_HANDLERS` coverage invariant,
  and full `register_adapter` / `unregister_adapter` IPC round-trips
  (ok / toml_override / missing-field / each unregister skip reason /
  snapshot-at-dispatch-time semantics).
- **`tests/end_to_end.rs` (integration, tokio).** A hand-rolled Python
  mock adapter exercises the full session lifecycle (spawn → initialize
  handshake + capabilities → launch + `initialized` event →
  setBreakpoints verified replies + breakpoint cache snapshot →
  configurationDone + `stopped` event → threads → stackTrace → continue
  + `terminated` event → graceful disconnect), and `drain_events`
  batch delivery. A BL-081 live-smoke test drives a real upstream
  `debugpy.adapter` for the initialize handshake + capability
  negotiation + clean disconnect. Both Python-dependent tests skip
  silently when `python3` / `debugpy` is unavailable so CI stays green.
```
