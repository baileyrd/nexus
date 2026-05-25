# nexus-lsp

> Kind: lib · IPC plugin id: com.nexus.lsp · CorePlugin: yes · Has settings: LspHostConfig · As of: 2026-05-25

## Overview

`nexus-lsp` is the **LSP host** — the core plugin (`com.nexus.lsp`) that spawns external Language Server Protocol servers as child processes, bridges their JSON-RPC-over-stdio streams to the kernel IPC surface, and republishes server-pushed notifications (chiefly `publishDiagnostics`) on the kernel event bus. It is the LSP analogue of `nexus-mcp`: a [`ConnectionPool`](#position-in-the-dependency-graph) holds at most one [`LspClient`] per configured server, each lazily connected on first use and reconnected with exponential backoff on transient failure. The crate description sums it up: "spawns external Language Server Protocol servers and bridges JSON-RPC to the kernel IPC bus."

The host is a **transparent proxy**. Most IPC verbs (`completions`, `hover`, `definition`, `references`, `rename`, `code_actions`, `format`, `execute_command`) forward a `serde_json::Value` payload straight to the routed upstream server and return the raw response. Only `open_file` / `close_file` / `change_file` need protocol awareness: they translate IPC arguments into the LSP `textDocument/did{Open,Close,Change}` shape and update per-document state so a crashed server can re-synchronise its open-document set on reconnect. The file path on every per-document verb is the *routing hint* — [`LspHostConfig::server_for_path`] picks the configured server by file extension; a path that matches no server returns JSON `null` rather than an error.

Servers are configured two ways. The primary path is `<forge>/.forge/lsp.toml`, an array-of-tables parsed by [`LspHostConfig::read_from`]. The second is the **BL-113 / ADR 0027 contribution model**: plugins may declare language-server definitions in their manifest, and `nexus-bootstrap`'s wiring helper (`lsp_contribution_wiring::wire_lsp_contributions`) calls the `register_server` / `unregister_server` IPC verbs at plugin activate/deactivate time. Both kinds of entry share one in-memory `servers` map; a parallel `contributed_by` map records the contributing plugin's reverse-DNS id so the host can (a) refuse a plugin's `unregister_server` against a TOML-pinned name and (b) refuse one plugin from unregistering another plugin's server. Precedence is **TOML wins**: a contributed adapter colliding with a TOML name is reported as skipped (`status = "toml_override"`).

Microkernel fit: `nexus-lsp` is a subsystem crate. It depends on the kernel (`nexus-kernel`) and the plugin contract (`nexus-plugins`), never the reverse. Every capability — completions, diagnostics, server lifecycle — reaches CLI / TUI / MCP / shell through one path: `context.ipc_call("com.nexus.lsp", command, args)`. The kernel never depends on the LSP crate.

## Position in the dependency graph

- **Direct `nexus-*` deps:** `nexus-kernel` (for `EventBus` / `publish_plugin`), `nexus-plugins` (for the `CorePlugin` / `CorePluginFuture` / `PluginError` contract).
- **Notable external deps:** `tokio` (process spawn, async pipes, mpsc/oneshot/Mutex/RwLock, timeouts), `serde_json` + `serde` (wire types), `toml` (config parse), `thiserror` (error enums), `schemars` (JSON Schema for IPC mirror types), `tracing` (logging), and `ts-rs` (optional, behind the `ts-export` feature — gated so the production build does not pull it, mirroring `nexus-mcp` / `nexus-storage`).
- **Dev deps:** `tempfile`, `tokio` with `test-util`.
- **Crates depending on it:** `nexus-bootstrap` (registers the plugin via `plugins/lsp.rs`, drives contributions via `lsp_contribution_wiring.rs`, and constructs `LspServerSpec` values for the merge API — the reason `config` is a `pub mod`).

## Public API surface

**Crate root re-exports** (`lib.rs`): `LspClient`, `LspClientError`, `OpenDocument`, `LspConfigError`, `LspHostConfig`, `LspServerSpec`, `LspMergeSkip` (alias of `config::MergeSkip`), `LspMergeSkipReason` (alias of `MergeSkipReason`), `LspUnregisterError` (alias of `UnregisterError`), `LspCorePlugin`, `ConnectionPool`, `PoolConfig`.

### `config` (pub mod) — `lsp.toml` parser + BL-113 merge API
- `LspServerSpec` — one configured server: `name`, `command`, `args`, `file_types`, `root_markers`, `disabled`, `env`. `#[serde(deny_unknown_fields)]`.
- `LspHostConfig` — parsed servers (`servers: HashMap<name, spec>`) plus the `contributed_by: HashMap<name, plugin_id>` provenance map.
  - `read_from(&Path)` — parse `lsp.toml`; missing file → empty config (`Ok(default)`).
  - `server_for_path(&str)` — route by case-insensitive file extension, skipping disabled servers.
  - `merge_contributed(Vec<(spec, plugin_id)>) -> Vec<MergeSkip>` — batch register; TOML wins on collision.
  - `register_contributed(spec, plugin_id) -> Result<(), MergeSkipReason>` — single-spec insert; the `register_server` IPC entry point.
  - `unregister_contributed(name, plugin_id) -> Result<LspServerSpec, UnregisterError>` — owner-gated removal; the `unregister_server` IPC entry point.
- `MergeSkip` / `MergeSkipReason` (`TomlOverride`, `InvalidName`, `InvalidCommand`) — per-contribution skip diagnostics.
- `UnregisterError` (`NotFound`, `TomlEntry`, `NotOwnedByPlugin { actual_owner }`).
- `LspConfigError` (`Io`, `Parse`, `DuplicateServer`, `MissingField`).

### `transport` (private mod) — framed JSON-RPC codec
- `JsonRpcMessage` — untagged enum: `Request` / `Response` / `Notification`. Plus `JsonRpcRequest`, `JsonRpcResponse`, `JsonRpcError`, `JsonRpcNotification`.
- `read_message(&mut BufReader<R>)` — parse one `Content-Length`-framed message; 16 MiB body cap.
- `write_message(&mut W, &JsonRpcMessage)` — serialise + frame + flush.
- `TransportError` (`Io`, `BadHeader`, `BadBody`, `Eof`).

### `client` (private mod, selectively re-exported) — one client = one child process
- `LspClient` — live connection. `connect` (spawn + handshake), `send_request`, `send_notification`, `did_open` / `did_change` / `did_close`, `drain_notifications`, `next_notification`, `documents_snapshot`, `server_name`, `spec`, `is_alive`, `shutdown`.
- `OpenDocument` — public snapshot of one tracked document for the pool's resync path.
- `ServerNotification` — `{ method, params }` forwarded from the reader task.
- `LspClientError` (`Spawn`, `Handshake`, `Transport`, `ServerError`, `RequestTimeout`, `NotRunning`) with `is_transient()`.

### `pool` (pub mod) — connection pool
- `ConnectionPool` — `new`, `get_or_connect` (lazy connect), `call_with_reconnect` (transient-retry + document resync), `disconnect`, `shutdown_all`, `connected_servers`.
- `PoolConfig { backoff: Vec<Duration> }`, `default_backoff()` — `[100ms, 500ms, 2s, 10s, 30s]`.

### `core_plugin` (pub mod) — the `CorePlugin` impl
- `LspCorePlugin` — `new(forge_root, Option<Arc<EventBus>>)`; implements `CorePlugin` (`on_init` / `on_start` / `on_stop` / `dispatch` / `dispatch_async`).
- `PLUGIN_ID` (`"com.nexus.lsp"`), `HANDLER_*` constants, and `IPC_HANDLERS: &[(&str, u32)]` (SD-06 single source of truth consumed by bootstrap).

### `ipc` (pub mod) — wire-mirror schema types
Concrete `Serialize`/`Deserialize`/`JsonSchema` (+ optional `TS`) structs the schema generator and shell consume, mirroring the ad-hoc `json!` shapes the handlers emit: `LspOpenFileArgs`, `LspPathArgs`, `LspChangeFileArgs`, `LspPositionArgs`, `LspReferencesArgs`, `LspRenameArgs`, `LspCodeActionsArgs`, `LspExecuteCommandArgs`, `LspServerEntry`, `LspOpenFileReply`, `LspOk`, `LspRegisterServerArgs`, `LspRegisterServerReply`, `LspUnregisterServerArgs`, `LspUnregisterServerReply`.

## IPC handlers

14 handlers. `list_servers`, `register_server`, `unregister_server` run on the synchronous `dispatch` path; handlers 2–12 run on `dispatch_async` (they touch child-process I/O). Handlers 2–12 return JSON `null` when the path routes to no configured server.

| # | command | args | returns | capability | description |
|---|---------|------|---------|------------|-------------|
| 1 | `list_servers` | `{}` | array of `{name, command, args, file_types, disabled}` | — | List configured servers (TOML + contributed), from the in-memory config. Read-only. |
| 2 | `open_file` | `{path, content, language_id?, version?}` | `{uri, server}` or `null` | — | `textDocument/didOpen`; tracks document state. `language_id` inferred from extension if absent; `version` defaults `1`. |
| 3 | `close_file` | `{path}` | `{ok:true}` or `null` | — | `textDocument/didClose`; drops tracked state. |
| 4 | `change_file` | `{path, content, version}` | `{ok:true}` or `null` | — | `textDocument/didChange` (full-sync); updates tracked text/version. |
| 5 | `completions` | `{path, line, character}` | raw LSP result or `null` | — | Proxies `textDocument/completion`. |
| 6 | `hover` | `{path, line, character}` | raw LSP result or `null` | — | Proxies `textDocument/hover`. |
| 7 | `definition` | `{path, line, character}` | raw LSP result or `null` | — | Proxies `textDocument/definition`. |
| 8 | `references` | `{path, line, character, include_declaration?}` | raw LSP result or `null` | — | Proxies `textDocument/references`. `include_declaration` defaults `true`. |
| 9 | `rename` | `{path, line, character, new_name}` | raw LSP `WorkspaceEdit` or `null` | — | Proxies `textDocument/rename`. |
| 10 | `code_actions` | `{path, range}` | raw LSP result or `null` | — | Proxies `textDocument/codeAction`. `range` defaults to a zero-zero range; `context.diagnostics` sent empty. |
| 11 | `format` | `{path}` | raw LSP result or `null` | — | Proxies `textDocument/formatting` with `{tabSize:4, insertSpaces:true}`. |
| 12 | `execute_command` | `{path, command, arguments?}` | raw LSP result or `null` | — | Proxies `workspace/executeCommand`. `path` is a routing hint only; powers the BL-077 follow-up (command-only code actions). `arguments` defaults `[]`. |
| 13 | `register_server` | `{name, command, args?, file_types?, root_markers?, disabled?, env?, plugin_id}` | `{ok, status}` (`status` ∈ `ok`/`toml_override`/`invalid_name`/`invalid_command`) | (intended `protocol.host.contribute`; see note) | BL-113 Phase 2b — register a plugin-contributed server. Validation failures are returned as a non-`ok` status, not a `PluginError`. |
| 14 | `unregister_server` | `{name, plugin_id}` | `{ok, status, actual_owner?}` (`status` ∈ `ok`/`not_found`/`toml_entry`/`not_owned_by_plugin`) | (intended `protocol.host.contribute`; see note) | BL-113 Phase 2b — owner-gated removal of a contributed server. `actual_owner` populated on `not_owned_by_plugin`. |

> `dispatch_async` returns `None` (so the kernel treats the call as bad-args) when a required field is absent — e.g. a `hover` call with no `path` or no integer `line`. Bootstrap registers these names plus their `v1.*` aliases via `with_v1_aliases`.

## Capabilities

The crate **declares and checks no capabilities inside the handlers themselves**. The `CorePlugin` impl does not call any capability gate; per-document and proxy verbs run unconditionally once routing succeeds.

`docs/0.1.2/ipc-handlers.md` records the *intended* gating: `register_server` / `unregister_server` should require `protocol.host.contribute` (invoker-only contribution lifecycle), and process-spawn for a launched language server rides on `process.spawn`. The source is explicit that this is **not yet enforced at the verb level**: `handle_register_server`'s doc-comment cites ADR 0027 §Open Question #3 — "no capability gate at the verb level … runtime usage capabilities (`process.spawn`) ride on the contributing plugin's existing grants and are checked at the `start` boundary, not here. Hard enforcement at the verb level is filed as a hardening follow-up." Process spawning itself uses `tokio::process::Command` directly in `LspClient::connect` with no in-crate capability check.

**Gap:** the `protocol.host.contribute` / `process.spawn` columns in `ipc-handlers.md` describe design intent, not code that exists in this crate today.

## Settings / Config

`LspHostConfig` is loaded from `<forge>/.forge/lsp.toml` by `on_init`. A missing or unparseable file leaves the host with zero servers (it logs and continues — never fails plugin init). The schema is array-of-tables (chosen over `mcp.toml`'s keyed-map shape because an LSP server name maps 1:1 to a command-line tool):

```toml
[[servers]]
name = "rust-analyzer"          # required, unique; the IPC routing key
command = "rust-analyzer"       # required; looked up on $PATH if not absolute
args = []                       # optional, default []
file_types = ["rs"]             # optional; extensions (no dot) this server handles
root_markers = ["Cargo.toml"]   # optional; marker files for rootUri resolution
disabled = false                # optional, default false — keeps entry, skips spawn

[servers.env]                   # optional; merged onto host process env at spawn
RUST_LOG = "error"
```

**Field defaults:** `args`, `file_types`, `root_markers`, `env` all default empty; `disabled` defaults `false`. **Validation** (in `read_from` and `register_contributed`): empty/whitespace `name` → `MissingField`/`InvalidName`; empty/whitespace `command` → `MissingField`/`InvalidCommand`; duplicate `name` in TOML → `DuplicateServer`.

`PoolConfig.backoff` is *not* surfaced in `lsp.toml`; it is constructed from `default_backoff()` (`[100ms, 500ms, 2s, 10s, 30s]`) and is hardcoded in `LspCorePlugin::new`.

> Note: `root_markers` is parsed, stored, and documented, but the init handshake currently always uses the **forge root** as `rootUri` / `workspaceFolders` (`LspClient::initialize`). The per-file root-marker walk described in the field's own doc-comment is not yet wired into `connect`. **Gap.**

## Events

**Published** (via `EventBus::publish_plugin`):
- `com.nexus.lsp.started` — on `on_start`, payload `{configured_servers: <n>}`.
- `com.nexus.lsp.<lsp_method_with_slashes_replaced_by_dots>` — every server-pushed notification fans out here with the original LSP `params` verbatim. The canonical case is `com.nexus.lsp.textDocument.publishDiagnostics` (diagnostics delivery). `$/progress`, `window/logMessage`, etc. fan out the same way.

There is no dedicated background polling task. Notifications are drained and republished opportunistically by `republish_pending` on every async handler call (`open_file`, position requests, etc.) — chatty enough to keep diagnostic latency low without competing with kernel shutdown.

**Subscribed:** none.

## Internals & notable implementation details

**Process spawning + lifecycle.** `LspClient::connect` runs `tokio::process::Command` with all three stdio streams piped and `kill_on_drop(true)`, `current_dir(forge_root)`, and the spec's `env` merged. A detached task drains the child's stderr into `tracing::debug` so a misbehaving server can't block its pipe. A reader task owns the child's stdout. On `on_stop`, the plugin spawns a current-thread tokio runtime on a dedicated OS thread, calls `pool.shutdown_all()`, and polls for completion against a 5 s `SHUTDOWN_DEADLINE`; on timeout it logs an `audit = true` warning and abandons the join (children may be stranded until the host process exits). `LspClient::shutdown` is the graceful path: `shutdown` request (2 s cap) → `exit` notification → close stdin → wait up to 5 s for the child, killing it otherwise.

**Content-Length JSON-RPC framing** (`transport`). `read_message` parses `Key: Value\r\n` header lines (CRLF or bare-LF tolerated, `Content-Length` matched case-insensitively, `Content-Type` and others accepted-and-ignored), then reads exactly `Content-Length` body bytes and deserialises into the untagged `JsonRpcMessage`. EOF before any header byte → `TransportError::Eof` (the canonical "child exited" path); a 16 MiB body cap (`MAX_BODY_BYTES`) guards against a runaway server OOMing the host. `write_message` emits `Content-Length: N\r\n\r\n` + body and flushes.

**Initialize handshake + capability negotiation.** `initialize` sends `processId` (host pid), `clientInfo`, `rootUri`/`workspaceFolders` (forge root), and a `minimal_client_capabilities()` object covering completion, hover, definition, references, rename, codeAction, formatting, publishDiagnostics, plus workspace `configuration`/`workspaceFolders`/`didChangeConfiguration` and window `showMessage`/`workDoneProgress`. The `initialize` deadline is 30 s (`INITIALIZE_TIMEOUT`, vs the 10 s `DEFAULT_REQUEST_TIMEOUT`) to absorb rust-analyzer cold-start. The server's returned capabilities are logged but **not** used to constrain the host (the shell client validates). `initialized` is sent immediately after.

**Request / notification routing.** Outbound requests get a monotonic integer `id` (`AtomicI64`), register a `oneshot` sender in a shared `pending` map, write the framed request under the stdin mutex, and await the response with a timeout (late responses are evicted from `pending`). The reader task demultiplexes inbound messages: `Response` → match `pending` by `id` and complete the oneshot (Ok result or `ServerError`); `Notification` → `try_send` to a bounded (`1024`) mpsc channel, dropping excess with a single latched warn per saturation episode so a chatty server can't wedge the client; `Request` (server-initiated) → `build_server_request_reply` synthesises a spec-compliant no-op reply and writes it back over stdin. On EOF or transport error the reader drains every pending request with a synthetic JSON-RPC error (`-32000`/`-32001`) so no caller hangs.

**Server-initiated request replies (BL-076).** `build_server_request_reply` / `build_known_reply` form a pure, unit-tested dispatch table answering the methods major servers issue at boot: `workspace/configuration` → array of `null` matching the requested `items` count (rust-analyzer treats a length mismatch as fatal); `workspace/workspaceFolders` → `null`; `window/showMessageRequest` → `null` (canceled); `window/showDocument` → `{success:false}`; `window/workDoneProgress/create` / `client/(un)registerCapability` / the `*/refresh` family → `null`; `workspace/applyEdit` → `{applied:false}`; everything else → `-32601` method-not-found. Without this, servers relying on `workspace/configuration` would hang waiting on stdin.

**Diagnostics delivery.** Server `publishDiagnostics` notifications travel through the reader task's mpsc channel → `drain_notifications` → `republish_pending`, which rewrites the LSP method (`textDocument/publishDiagnostics`) to a dotted bus topic (`com.nexus.lsp.textDocument.publishDiagnostics`) and publishes the verbatim `params`.

**Pool, reconnect & document resync.** `get_or_connect` lazy-connects (a connect race prefers the already-inserted entry, letting the loser's child be reaped by `kill_on_drop`). `call_with_reconnect` runs the op closure across `1 + backoff.len()` attempts: non-transient errors short-circuit immediately (so handshake / method-not-found don't burn the budget); transient ones (`Transport`, `NotRunning`, `RequestTimeout`) snapshot the broken client's open-document set (`documents_snapshot`), drop the entry, sleep the backoff step, and on the next attempt replay every document via `did_open` against the fresh connection before re-running the op. Per-server concurrency is mutex-serialised (`Arc<Mutex<LspClient>>`), matching the `nexus-mcp` shape.

**Config snapshotting (BL-113).** The active config lives behind `Arc<RwLock<LspHostConfig>>` so `register_server` / `unregister_server` can mutate it at runtime. Each async dispatch takes an immutable `Arc<LspHostConfig>` snapshot at dispatch time, so an in-flight command keeps the server view it started with even if a concurrent contribution mutates the master config.

## Tests

- **Unit tests in `config.rs`** — TOML parse (missing file → empty, two-server block, duplicate-name error, empty-command rejection), `server_for_path` (case-insensitive extension match, skips disabled), and the full BL-113 surface: `merge_contributed` (insert, TOML-wins collision, invalid name/command rejection, input-order preservation, `contributed_by` population), `register_contributed` (happy path, collisions), `unregister_contributed` (owner match, and the `NotFound`/`TomlEntry`/`NotOwnedByPlugin` distinction).
- **Unit tests in `transport.rs`** — request round-trip, response parse, notification with extra `Content-Type` header, EOF-before-byte → `Eof`, missing `Content-Length` → `BadHeader`, oversized-message cap.
- **Unit tests in `client.rs`** — `file_uri` scheme; the complete BL-076 server-initiated reply table (per-method shape assertions incl. `workspace/configuration` array-count contract and empty/missing-items edge cases, unknown-method `-32601`, id round-trip for int/string forms); `is_transient` classification.
- **Unit tests in `core_plugin.rs`** — `PLUGIN_ID`, `on_init` (no TOML / valid TOML / invalid TOML → empty), `list_servers` array shape, unknown-handler error, async-handler-missing-args → `None`, `file_uri_from_path` passthrough, `infer_language_id`, `on_start`/`on_stop` safety, and the BL-113 `register_server` / `unregister_server` IPC round-trips (insert+ok, TOML-collision reject, missing-field error, owner-match removal, each skip-reason status).
- **`tests/end_to_end.rs`** — integration against a tiny Python (`python3`) mock LSP server; **silently skipped if `python3` is absent**. Covers: spawn + `initialize`/`initialized` handshake, hover request→response correlation, `didChange` → `publishDiagnostics` fan-out, `documents_snapshot` open/close tracking, `call_with_reconnect` replaying open documents after a forced transient failure, and (via a second "config probe" mock) the BL-076 `workspace/configuration` server-initiated request being answered with `[null, null]`.

**Coverage gaps:** no test exercises a real language server (rust-analyzer etc.); `root_markers`-based root resolution is untested because it isn't wired; capability gating is untested because none is enforced in-crate.
