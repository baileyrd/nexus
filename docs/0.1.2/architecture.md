# Architecture

> **As of:** 2026-05-17. Derived from `Cargo.toml`, `crates/nexus-bootstrap/src/lib.rs`, `crates/nexus-bootstrap/tests/dep_invariants.rs`, and `crates/nexus-bootstrap/cap_matrix.toml`.

## Shape

Nexus is a Rust **microkernel** workspace (38 crates) plus a pnpm workspace (`shell/` Tauri 2 app + `packages/nexus-extension-api` TypeScript SDK). One CLI binary (`nexus`), one TUI binary (`nexus-tui`), one desktop shell (`nexus-shell`), one MCP server (subcommand of `nexus`).

The kernel mediates **every cross-subsystem call** through one path:

```
context.ipc_call(plugin_id, command, args) -> Result<serde_json::Value>
```

Around ~280 IPC handlers across ~25 in-tree plugin ids ([`ipc-handlers.md`](ipc-handlers.md)). 30 capabilities ([`capabilities.md`](capabilities.md)). Capability checks run unconditionally at dispatch.

## Four invariants

Authoritative source: `crates/nexus-bootstrap/tests/dep_invariants.rs`. The test fails CI if any are violated.

### 1. File-as-truth

Markdown files on disk are authoritative. `.forge/index.db` (SQLite) and `.forge/search/` (Tantivy) are rebuildable derived state. `nexus-storage` owns the file watcher; the index is regenerable from files but not vice versa.

### 2. Microkernel isolation

`nexus-kernel` depends only on `nexus-types` and `nexus-plugin-api` (both leaf crates). Subsystem crates may depend on the kernel; the kernel never depends on a subsystem.

Enforced FORBIDDEN dependency list (`dep_invariants.rs:17`):

```
nexus-cli       ⛔ nexus-storage
nexus-tui       ⛔ nexus-storage
nexus-ai        ⛔ nexus-storage
nexus-mcp       ⛔ nexus-storage, nexus-ai, nexus-ai-runtime
nexus-database  ⛔ nexus-storage, rusqlite
nexus-cli       ⛔ nexus-database, nexus-ai-runtime
nexus-tui       ⛔ nexus-database, nexus-ai-runtime
nexus-acp       ⛔ nexus-agent, nexus-ai, nexus-storage
nexus-remote    ⛔ nexus-agent, nexus-ai, nexus-storage
nexus-kernel    ⛔ rusqlite, nexus-kv
```

Rationale per pair is in the inline comments — most reduce to "ipc_call instead of linking; bootstrap is the sole linker".

### 3. IPC over direct calls

CLI, TUI, MCP server, and the Tauri shell all reach storage / AI / editor / etc. through `ipc_call`. The Tauri bridge in `shell/src-tauri/src/lib.rs` is intentionally thin — see [`shell.md`](shell.md). New backend capability ⇒ new IPC handler in the right service crate, not a new direct dependency from a frontend.

**Timeout & cancellation semantics (#200 / R17).** `ipc_call` enforces a per-dispatch deadline (`IpcError::Timeout`) and exposes a cooperative `CancellationToken` to the handler (`IpcError::Cancelled`). For **async** handlers (`dispatch_async`) the kernel races the future against the timeout/cancel token in a `tokio::select!`, so both fire promptly. For **sync** handlers (the default `dispatch` path running on `spawn_blocking`), the deadline releases the *caller* and frees its blocking-pool *wait*, but Rust offers no preemption inside the blocking body — the handler keeps running until it either returns or polls `nexus_kernel::ipc_cancel_token().is_cancelled()` at a safe yield point. Long-running sync handlers should poll the token, chunk their work, or convert to `dispatch_async`. See `crates/nexus-kernel/src/context_impl.rs:226-262`.

### 4. Capabilities gate everything

`fs.read`, `fs.write`, `net.http`, `process.spawn`, `ipc.call`, `events.publish`, `ai.chat`, etc. Every kernel-mediated operation checks a capability before it runs. The full inventory + risk classification is at [`capabilities.md`](capabilities.md). Per-handler capability assignment is at [`ipc-handlers.md`](ipc-handlers.md), declared once in `crates/nexus-bootstrap/cap_matrix.toml`.

## Boot order

`build_cli_runtime(forge_root)` / `build_tui_runtime(forge_root)` / `init_forge(forge_root)` in `crates/nexus-bootstrap/src/lib.rs` assemble a `Runtime`. The bootstrap is the **only** crate that links every service plugin; every other consumer routes through IPC.

Registered core plugins (deterministic order — see `crates/nexus-bootstrap/src/plugins/mod.rs`):

1. `com.nexus.security` — keyring, audit log, TLS pinning
2. `com.nexus.storage` — file-as-truth + SQLite + Tantivy + watcher + graph + bases
3. `com.nexus.formats` — pure (de)serialization of markdown, canvas, notion archives
4. `com.nexus.database` — pure-compute formula/rollup/view (no SQL)
5. `com.nexus.editor` — block-tree, transactions, CM6 sessions
6. `com.nexus.terminal` — PTY sessions, REPL, ad-hoc + saved commands
7. `com.nexus.git` — libgit2 over the forge root
8. `com.nexus.ai` — provider traits, embeddings, RAG, tool loop
9. `com.nexus.ai.runtime` — task scheduler / observation surface (ADR 0028)
10. `com.nexus.agent` — agent archetypes, plan/run, transcript history
11. `com.nexus.skills` — `.skill.md` registry + render/compose/invoke
12. `com.nexus.templates` — `.template.md` registry + render/apply
13. `com.nexus.workflow` — `.workflow.toml` registry + cron + file_event triggers
14. `com.nexus.comments` — block-anchored comment threads
15. `com.nexus.linkpreview` — OG/Twitter-card fetcher
16. `com.nexus.notifications` — desktop / Discord / Telegram / email + inbox
17. `com.nexus.theme` — CSS variable registry, snippet cascade
18. `com.nexus.mcp.host` — connects to external MCP servers
19. `com.nexus.lsp` — LSP host (BL-076)
20. `com.nexus.dap` — DAP host (BL-081)
21. `com.nexus.acp` — ACP host (BL-144)
22. `com.nexus.audio` — STT/TTS provider traits
23. `com.nexus.collab` — WebSocket relay (BL-143)

## Crate dep graph (top of pyramid)

```
                              ┌────────────────┐
                              │ nexus-cli /tui │           frontends
                              └───────┬────────┘
                                      │
                              ┌───────▼────────┐
                              │ nexus-bootstrap│           the only linker of every plugin
                              └───────┬────────┘
        ┌───────┬──────┬──────────┬──┴───┬──────┬──────┬───────┬──────┐
        ▼       ▼      ▼          ▼      ▼      ▼      ▼       ▼      ▼
     storage   ai    editor   terminal  agent  git  workflow  ...    23 service crates
        │       │      │          │      │      │      │       │
        └───────┴──────┴──────────┴──────┴──────┴──────┴───────┘
                          ┌───────▼────────┐
                          │  nexus-kernel  │              leaf-only deps
                          └───────┬────────┘
                                  │
                  ┌───────────────┼───────────────┐
                  ▼                               ▼
            nexus-types                    nexus-plugin-api
```

Full per-crate detail in [`crates.md`](crates.md).

## Where things live

| Tree | What |
|------|------|
| `crates/nexus-kernel/` | Event bus, IPC dispatcher, capability system, plugin lifecycle, KV trait |
| `crates/nexus-plugins/` | Plugin loader, manifest, WASM sandbox (wasmtime), hot-reload, settings |
| `crates/nexus-storage/` | File-as-truth, SQLite, Tantivy, watcher, knowledge graph, bases SQL |
| `crates/nexus-<service>/` | 22 service-plugin crates — see [`crates.md`](crates.md) |
| `crates/nexus-bootstrap/` | Wires core plugins; `build_*_runtime`, `init_forge`, cap_matrix loader |
| `crates/nexus-cli/` | `nexus` binary (clap) |
| `crates/nexus-tui/` | `nexus-tui` library + binary (ratatui) |
| `crates/nexus-mcp/` | MCP server library (rmcp) + host client |
| `shell/` | Tauri 2 desktop shell, `shell/src/` React frontend, `shell/src-tauri/` Rust bridge |
| `packages/nexus-extension-api/` | `@nexus/extension-api` — TypeScript SDK for shell plugins |
| `scripts/check_ipc_drift.sh` | Regenerates TS bindings + JSON schemas + `docs/generated/capabilities.md` |

## How a request flows

Example: `nexus content read notes/foo.md` from the CLI.

1. CLI parses argv via clap → `commands::content::read`.
2. CLI obtains a `Runtime` via `nexus_bootstrap::build_cli_runtime(forge_root)`.
3. CLI calls `runtime.context.ipc_call("com.nexus.storage", "read_file", json!({"path": "notes/foo.md"}))`.
4. `nexus-kernel`'s `IpcDispatcher` looks up the handler by `(plugin_id, command_id)`.
5. Capability check: `ipc.call` (unconditional) + `cap_matrix.toml` row for `com.nexus.storage::read_file` (unrestricted in this case; downstream `fs.read` check applies if the path resolves external).
6. Dispatch reaches `StorageCorePlugin::dispatch(handler_id, args)` in `crates/nexus-storage/src/core_plugin.rs`.
7. Storage routes to its file reader; rejects paths outside `<forge>/` per `fs.read.external` gate.
8. Returns `serde_json::Value` → JSON response printed by CLI.

The TUI, MCP, and shell take the same path with a different transport in front of step 3.

## How to add a backend capability (the right way)

Symptom — "I want a new thing the frontend can do".

1. Pick the **service crate** that owns the domain (storage / ai / agent / git / etc.). If none exists, add a new `nexus-<service>` crate under `crates/`, register it from `nexus-bootstrap`.
2. Add a new handler id in the crate's `core_plugin.rs` `dispatch` match.
3. Add a `[[handler]]` row in `crates/nexus-bootstrap/cap_matrix.toml` — either `caps = […]` (if it does network/process/etc.) or `unrestricted = "<why>"`.
4. Run `cargo test -p nexus-bootstrap --test cap_matrix_complete -- --ignored` to confirm the matrix covers the new handler.
5. If the request/response carries new types, derive `#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]` and add the crate to `scripts/check_ipc_drift.sh`. Then `scripts/check_ipc_drift.sh` to regenerate `packages/nexus-extension-api/src/generated/ipc/*.ts` + JSON Schemas + `docs/generated/capabilities.md`.
6. Frontend calls it via `ctx.ipc_call(plugin_id, command, args)`. No direct dependency.

## How to add a setting (the right way)

1. Identify the right config surface — see [`settings/README.md`](settings/README.md).
2. Add a field to the relevant `Config` struct with a `Default` impl and a `serde(default = "…")` attribute.
3. If user-facing, register a `SettingsSchema` in the owning plugin so the settings UI surfaces it.
4. **Do not** introduce a new TOML file without a corresponding row in [`settings/README.md`](settings/README.md).
5. Remove the corresponding entry from [`settings/hardcoded-rust.md`](settings/hardcoded-rust.md) / [`settings/hardcoded-shell.md`](settings/hardcoded-shell.md) once promoted.
