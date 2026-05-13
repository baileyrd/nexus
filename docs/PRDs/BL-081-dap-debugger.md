# BL-081 — DAP debugger integration

**Status**: Specced + first-cut implementation 2026-05-13. Live-smoke against `codelldb` / `js-debug` deferred to operator step (binaries aren't on this dev box).
**Effort**: Large (4–6 weeks committed in the original entry; this first cut is the protocol host + IPC surface + shell control panel; per-language adapter convenience presets, watch-expression UX polish, and live-smoke loop deferred to follow-ups).
**Crates**: new `nexus-dap`; new `shell/src/plugins/nexus/debugger/`
**Related**: BL-076 (`nexus-lsp` — shipped 2026-05-07; this entry follows the same protocol-host pattern), BL-075 (code-mode editor), Debug Adapter Protocol (DAP) §1–§6

## Why

DAP is the debugger equivalent of LSP. The microkernel architecture invariant — "new capability ⇒ new IPC handler in the right service crate, not a new direct dependency from a frontend" — means a debugger plugin has to land as a Rust core plugin that talks to external debug adapters (`codelldb`, `js-debug`, `delve`, `debugpy`, …) over the same stdio-JSON-RPC envelope LSP uses, then surfaces every operation as an IPC verb on `com.nexus.dap`. Without it, code editing in Nexus is purely passive — `nexus-lsp` lights up completions / diagnostics / hover; `nexus-dap` lights up breakpoints / step / inspect.

## What DAP is, in two paragraphs

A debug adapter is an executable that speaks the same Content-Length-prefixed JSON envelope as LSP, but carries a slightly different message family: every message is one of `request`, `response`, or `event`, distinguished by a `"type"` discriminator (not by presence/absence of `id` like JSON-RPC). Adapter-side IDs are monotonic integers stored in `seq`; client requests assign their own `seq` and the adapter's `response` echoes it back as `request_seq`. Asynchronous notifications from the adapter (stopped, output, terminated, exited, thread, module, breakpoint changed, …) arrive as `event`-typed messages with a method name in `event` and a JSON body in `body`.

The session-level shape is: client `initialize` (capability negotiation) → client `launch` or `attach` (adapter starts the program or connects to a running one) → client `setBreakpoints` per source file → adapter emits `initialized` event → client `configurationDone` → adapter emits `stopped` events as breakpoints / steps land → client issues `stackTrace` / `scopes` / `variables` / `evaluate` against the current frame → client issues `continue` / `next` / `stepIn` / `stepOut` / `pause` → repeat → client `terminate` or `disconnect`.

## Scope of this BL

### In scope (first cut)

1. **`nexus-dap` core plugin** registered as `com.nexus.dap`, loads `<forge>/.forge/dap.toml`, lazy-spawns debug adapters, proxies the full request/response/event surface over IPC, republishes adapter-pushed events on `com.nexus.dap.<event>`.
2. **`com.nexus.dap` IPC surface**: 16 handlers covering the lifecycle (list_adapters, launch, configuration_done, disconnect, terminate), breakpoint management (set_breakpoints, set_function_breakpoints, set_exception_breakpoints), execution control (continue, next, step_in, step_out, pause), inspection (threads, stack_trace, scopes, variables, evaluate).
3. **Wire-mirror IPC types** for the schema generator + ts-rs bindings, same pattern as `nexus-lsp::ipc`.
4. **Bootstrap registration** in `nexus-bootstrap` next to `nexus-lsp`.
5. **Shell `nexus.debugger` plugin**: sidebar panel with Variables / Call Stack / Watch / Breakpoints sections, toolbar (Continue, Step Over, Step Into, Step Out, Pause, Stop, Restart), per-session state in a Zustand store, kernel IPC client for typed calls and event subscriptions.
6. **CM6 breakpoint gutter**: click in the gutter of any code-mode tab to set/clear a breakpoint, red-dot decoration, syncs through `set_breakpoints` per file.
7. **End-to-end mock-DAP test** (Python script identical in spirit to `nexus-lsp/tests/end_to_end.rs`) — initialize → launch → setBreakpoints → configurationDone → stopped event → stackTrace → continue → terminated event → disconnect.
8. **Unit tests** for transport framing, message envelope variants, request/response correlation, event dispatch, connection-pool reconnect with breakpoint replay.

### Deferred (named, scoped, but not built in this cut)

- **Live smoke against `codelldb` / `js-debug` / `debugpy` / `delve`** — same posture as `nexus-lsp` shipped with. The mock-Python adapter exercises every protocol path; live smoke is an operator step.
- **REPL widget for `evaluate` expressions** — the IPC handler is wired; a dedicated panel input that pushes expressions and renders the typed `Variable[]` results is its own UX scope. First cut renders evaluate results in the Watch section only.
- **Inline value decorations in CM6** (VSCode-style end-of-line display of locals at the current step) — needs a per-frame `variables` round-trip on every step and a separate CM6 decoration extension; deferred.
- **Conditional breakpoints / hit-count / logpoints** — the wire shape accepts these (extra fields on `SourceBreakpoint`); the UI to author them is deferred to a follow-up. First cut sets unconditional line breakpoints only.
- **Reverse debugging / time-travel** — adapter-specific, no UX investment yet.
- **Multi-session / multi-target** — pool supports it (keyed by adapter name like `nexus-lsp`); the shell UI assumes one active session at a time. A "Run and Debug" picker that lifts the multi-session case to first-class is a follow-up.

## Architecture (mirrors `nexus-lsp`)

```
┌──────────────────────────┐  ┌──────────────────────────┐
│  shell/nexus.debugger    │  │  CM6 breakpoint gutter   │
│  - DebuggerPanel.tsx     │  │  - clickable line marker │
│  - debuggerStore.ts      │  │  - syncs set_breakpoints │
│  - debuggerIpc.ts        │  └──────────────┬───────────┘
└──────────┬───────────────┘                 │
           │  com.nexus.dap::* IPC           │
           ▼                                 ▼
┌──────────────────────────────────────────────────────┐
│  nexus-dap (core plugin, com.nexus.dap)              │
│  ┌────────────┐  ┌────────────┐  ┌────────────────┐  │
│  │ core_plugin│→ │ ConnectionPool ↔ DapClient ×N │  │
│  └────────────┘  └────────────┘  └────────────────┘  │
│           │             ↑                            │
│           │ event-bus   │ stdio framed JSON          │
│  com.nexus.dap.<event>  ▼                            │
└──────────────────────────────────────────────────────┘
                          │
                          ▼
              ┌──────────────────────┐
              │ external adapter exe │
              │ codelldb / js-debug  │
              │ debugpy / delve / …  │
              └──────────────────────┘
```

### Module layout (matches `nexus-lsp`)

```
crates/nexus-dap/
├── Cargo.toml
├── src/
│   ├── lib.rs           # crate root + re-exports
│   ├── transport.rs     # Content-Length JSON framing (lifted from nexus-lsp; DAP envelope is the same)
│   ├── protocol.rs      # ProtocolMessage enum (Request | Response | Event) tagged by "type"
│   ├── config.rs        # <forge>/.forge/dap.toml parser
│   ├── client.rs        # one running adapter = one DapClient (spawn, handshake, request/response correlation, event channel)
│   ├── pool.rs          # ConnectionPool with reconnect + breakpoint replay
│   ├── core_plugin.rs   # com.nexus.dap CorePlugin
│   └── ipc.rs           # wire-mirror IPC arg/reply types for schema generator
└── tests/
    └── end_to_end.rs    # mock-Python adapter; full lifecycle
```

### Protocol envelope

DAP messages share the LSP-style Content-Length-prefixed frame. The body differs:

```json
{ "seq": 1, "type": "request", "command": "launch", "arguments": {…} }
{ "seq": 2, "type": "response", "request_seq": 1, "success": true, "command": "launch", "body": {…} }
{ "seq": 3, "type": "event", "event": "stopped", "body": {…} }
```

`seq` is a monotonic integer per direction (client and adapter each maintain their own). Responses correlate to requests via `request_seq`. Errors travel as `success: false` with `message: string` + optional `body.error` payload (vs LSP's `error: {code, message, data}` object).

### IPC surface (16 handlers)

| id | name | args | summary |
|---|---|---|---|
| 1 | `list_adapters` | — | dump configured `dap.toml` entries |
| 2 | `launch` | `{adapter, program, mode?, args?, cwd?, env?, stop_on_entry?, extra?}` | spawn adapter + send `launch` |
| 3 | `attach` | `{adapter, pid?, port?, extra?}` | spawn adapter + send `attach` |
| 4 | `configuration_done` | `{adapter}` | send `configurationDone` after breakpoints set |
| 5 | `disconnect` | `{adapter, terminate_debuggee?}` | clean shutdown |
| 6 | `terminate` | `{adapter}` | force-stop the debuggee, keep adapter alive |
| 7 | `set_breakpoints` | `{adapter, source_path, breakpoints: [{line, condition?, hit_condition?, log_message?}]}` | replace breakpoints for one source |
| 8 | `set_function_breakpoints` | `{adapter, breakpoints: [{name, condition?}]}` | function-name breakpoints |
| 9 | `set_exception_breakpoints` | `{adapter, filters: string[]}` | exception filters (`raised`, `uncaught`, …) |
| 10 | `continue` | `{adapter, thread_id}` | resume |
| 11 | `next` | `{adapter, thread_id}` | step over |
| 12 | `step_in` | `{adapter, thread_id}` | step in |
| 13 | `step_out` | `{adapter, thread_id}` | step out |
| 14 | `pause` | `{adapter, thread_id}` | request a stop |
| 15 | `threads` | `{adapter}` | list threads |
| 16 | `stack_trace` | `{adapter, thread_id, start_frame?, levels?}` | stack frames for thread |
| 17 | `scopes` | `{adapter, frame_id}` | scopes (Locals, Closure, Globals, …) for a frame |
| 18 | `variables` | `{adapter, variables_reference, filter?, start?, count?}` | resolve a `variablesReference` |
| 19 | `evaluate` | `{adapter, expression, frame_id?, context?}` | REPL / watch evaluation |

(IPC IDs are `1..=19`; the count of 16 in the original BL-081 entry was a planning estimate — the finalised surface is 19 because lifecycle naturally splits into more verbs than the original sketch.)

### Bus events

Every adapter event fans out as `com.nexus.dap.<event>` with the original `body` payload preserved verbatim. Known events:

| Event | Topic |
|---|---|
| `initialized` | `com.nexus.dap.initialized` |
| `stopped` | `com.nexus.dap.stopped` |
| `continued` | `com.nexus.dap.continued` |
| `exited` | `com.nexus.dap.exited` |
| `terminated` | `com.nexus.dap.terminated` |
| `thread` | `com.nexus.dap.thread` |
| `output` | `com.nexus.dap.output` |
| `breakpoint` | `com.nexus.dap.breakpoint` |
| `module` | `com.nexus.dap.module` |
| `process` | `com.nexus.dap.process` |
| `capabilities` | `com.nexus.dap.capabilities` |

Unknown event names pass through as `com.nexus.dap.<event>` unchanged.

### Config (`<forge>/.forge/dap.toml`)

```toml
[[adapters]]
name = "rust"                     # stable identifier used by IPC `adapter` arg
command = "codelldb"
args = ["--port", "0"]            # optional
type = "lldb"                     # optional cosmetic hint
file_types = ["rs", "c", "cpp"]   # optional — for "auto-pick adapter for this file" UX
disabled = false                  # optional

[adapters.env]                     # optional
RUST_BACKTRACE = "1"
```

Empty / missing `dap.toml` → host loads zero adapters, every IPC call returns `null` for the `adapter` it can't find. Same posture as `nexus-lsp`.

## Definition of done

- [x] `nexus-dap` crate compiles standalone (`cargo build -p nexus-dap`).
- [x] Transport round-trips request/response/event frames.
- [x] `DapClient::connect` runs the `initialize` handshake and reports adapter capabilities back to the caller.
- [x] All 19 IPC handlers are reachable via `dispatch_async` and proxy through the pool's reconnect loop.
- [x] Adapter events fan out on the kernel bus as `com.nexus.dap.<event>`.
- [x] `ConnectionPool` replays the last-known per-source breakpoint set against a reconnected adapter (mirror of `nexus-lsp`'s `did_open` resync).
- [x] `nexus-bootstrap` registers `com.nexus.dap` with manifest + handler aliases, lifecycle hooks fire in order.
- [x] `scripts/check_ipc_drift.sh` passes after regenerating ts-rs + schemars bindings.
- [x] `cargo test -p nexus-dap` ≥ 25 tests pass.
- [x] End-to-end mock-adapter test exercises the full lifecycle (Python mock script in `tests/end_to_end.rs`).
- [x] Shell `nexus.debugger` plugin renders a sidebar panel and toolbar against a live session.
- [ ] CM6 breakpoint gutter clickable, syncs through `set_breakpoints` — **deferred to a follow-up**. The store + IPC verb (`toggleBreakpoint`) exist; what's missing is a CM6 `gutter()` extension wired into the editor plugin's code-mode bundle. That's a cross-cutting change inside `shell/src/plugins/nexus/editor/cm/` which is best landed as its own targeted PR. Today the panel's Breakpoints section is read-only relative to source files.
- [x] `pnpm --filter nexus-shell test` stays green.
- [ ] Live smoke against `codelldb` / `js-debug` — operator step; the mock adapter covers every protocol path.

## Implementation plan (phased)

Each phase is one tractable PR with explicit exit criteria; phases are sequential because each builds on the previous one's wire types.

| Phase | Work | Exit criteria |
|---|---|---|
| **P1 — transport + protocol envelope** | `transport.rs` (Content-Length framing, MAX_BODY_BYTES=16 MiB), `protocol.rs` (`ProtocolMessage` enum with `#[serde(tag="type")]`, `Request`/`Response`/`Event` variants), unit tests for each variant + EOF / oversized / malformed-header rejection. | 8+ transport/protocol unit tests pass. |
| **P2 — config + client** | `config.rs` (TOML parser, dup-name guard, missing-required-fields guard, `adapter_for_path()`), `client.rs` (spawn, handshake, request/response correlation, event channel, breakpoint cache, graceful shutdown). | `cargo test -p nexus-dap` ≥ 18 tests pass, all client surface covered. |
| **P3 — pool + reconnect** | `pool.rs` (`ConnectionPool`, `PoolConfig`, `default_backoff`, `get_or_connect`, `call_with_reconnect`, `disconnect`, `shutdown_all`, `connected_adapters`); breakpoint replay across reconnect via cached `set_breakpoints` snapshot. | Pool tests + reconnect-with-replay tests pass. |
| **P4 — core plugin** | `core_plugin.rs` (`DapCorePlugin` + 19 `HANDLER_*` const ids + `dispatch_async` table), `ipc.rs` (wire-mirror types with `schemars` + ts-rs gating). | All 19 verbs reachable via `dispatch_async`; bus republish helper covered. |
| **P5 — bootstrap + schema emit** | Register in `nexus-bootstrap/src/lib.rs` next to `nexus-lsp`. Add wire types to `nexus-bootstrap/tests/ipc_schema_emit.rs`. Add `nexus-dap` to workspace `Cargo.toml`, `nexus-bootstrap/Cargo.toml`, ts-export feature. | `cargo build --workspace`, `cargo test -p nexus-bootstrap`, `scripts/check_ipc_drift.sh` all pass. |
| **P6 — end-to-end test** | `tests/end_to_end.rs` Python mock adapter, full lifecycle. | Test passes when `python3` is available; silently skipped otherwise. |
| **P7 — shell plugin: panel + IPC client** | `shell/src/plugins/nexus/debugger/` with `index.ts` (plugin registration), `debuggerIpc.ts` (typed wrappers around `api.ipc.invoke('com.nexus.dap', …)`), `debuggerStore.ts` (Zustand: active session, threads, stack frames, breakpoints, output log), `DebuggerPanel.tsx` (Variables / Call Stack / Watch / Breakpoints + toolbar). | Plugin loads, panel renders with empty state when no session is active. |
| **P8 — CM6 breakpoint gutter** | `cm/breakpointGutter.ts` extension wired into the editor plugin's code-mode bundle. Click toggles, decoration rendered, syncs through `set_breakpoints` for the file's open session. | Click-to-set-breakpoint works in a code-mode tab; the store reflects the breakpoint list. |
| **P9 — verify** | `cargo test --workspace`, `pnpm --filter nexus-shell test`, `scripts/check_ipc_drift.sh`. Update BACKLOG to close BL-081. | All test suites green; BACKLOG entry moved to the closed-list pattern. |

## Trade-offs and explicit decisions

- **Mirror `nexus-lsp` rather than build from scratch.** The transport, pool, reconnect, and bus-republish patterns are load-bearing and already proved out by BL-076. The variation is purely in the message envelope (DAP uses `"type":"request"|"response"|"event"` instead of JSON-RPC's `id` presence/absence), and the message family (request/response/event vs request/response/notification with no events).
- **No `nexus-dap → nexus-storage` dep.** The adapter handles its own file reads — Nexus only needs the source path to route `set_breakpoints` and to align gutter clicks. The microkernel invariant is preserved.
- **One adapter per process.** No process-pool sharing; each `launch`/`attach` creates a new adapter instance. DAP adapters are cheap to spawn and the per-adapter state (loaded modules, source maps, …) doesn't share well anyway.
- **Breakpoints are file-scoped, replaceable.** DAP's `setBreakpoints` semantically replaces the entire breakpoint list for a source on each call. The shell store keeps a per-file `Set<line>` and the adapter wire layer is stateless on top of that — no per-breakpoint id tracking.
- **Single active session in the UI.** The pool can hold many connections, but the shell's `debuggerStore` exposes only one "current" session at a time. A multi-session UI is a follow-up.
