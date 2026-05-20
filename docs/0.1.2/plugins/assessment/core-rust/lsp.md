# com.nexus.lsp

- **Path:** `crates/nexus-lsp/`
- **Tier:** Core Rust
- **Bootstrap order:** 19

## Architecture

- Entry point `crates/nexus-lsp/src/lib.rs` re-exports `LspCorePlugin`, `LspHostConfig`, `ConnectionPool`. Registered by `crates/nexus-bootstrap/src/plugins/lsp.rs` with `LifecycleFlags { on_init, on_start, on_stop }` all on.
- Key modules: `core_plugin` (IPC dispatch + lifecycle), `config` (TOML loader + BL-113 contributed-server map), `client` (stdio JSON-RPC framing), `pool` (per-server `ConnectionPool` with lazy connect + reconnect-with-backoff), `transport` (Content-Length framed JSON-RPC), `ipc` (wire-mirror types behind the `ts-export` feature).
- `on_init` reads `<forge>/.forge/lsp.toml`; a missing file yields an empty config (no error). Parse errors warn and leave the host disabled. `on_start` publishes `com.nexus.lsp.started` with `configured_servers` count. `on_stop` spawns a current-thread tokio runtime, calls `pool.shutdown_all()`, and hard-caps the join at 5s so a misbehaving server can't hang kernel shutdown.
- Persistence: `<forge>/.forge/lsp.toml` (documented at `docs/0.1.2/settings/forge-config.md:110`). No SQLite, no derived index. Per-server `OpenDocument` state lives in memory inside `LspClient` so a crashed server can resync via the reconnect loop.
- Settings owned: the `[[servers]]` array in `lsp.toml` (one block per language server, see `crates/nexus-lsp/src/config.rs:66`). No `[lsp]` block in `app.toml`. BL-113 plugin contributions land in `LspHostConfig::contributed_by` at runtime; the contribution wiring lives in `crates/nexus-bootstrap/src/lsp_contribution_wiring.rs`.
- External dependencies: spawns arbitrary child processes (the configured `command`), reads/writes their stdio. No network from this crate; servers themselves may open sockets. Pulls `tokio`, `toml`, `tracing`, no extra system libs.

## Surface

- IPC handlers (from `IPC_HANDLERS` in `core_plugin.rs:91`):
  - `list_servers` (1) — sync, returns configured server array.
  - `open_file` (2), `close_file` (3), `change_file` (4) — async, translate to LSP `textDocument/did{Open,Close,Change}`; update `LspClient` document state for reconnect resync.
  - `completions` (5), `hover` (6), `definition` (7), `references` (8), `rename` (9), `code_actions` (10), `format` (11), `execute_command` (12) — async, transparent proxies for the equivalent LSP request.
  - `register_server` (13), `unregister_server` (14) — sync, BL-113 / ADR 0027 plugin-contributed adapter management (precedence: TOML wins; unregister gated on `plugin_id` ownership).
- Bus events: server-pushed notifications re-emitted as `com.nexus.lsp.<lsp_method_with_dots>` (e.g. `com.nexus.lsp.textDocument.publishDiagnostics`). `com.nexus.lsp.started` on plugin start. No UI contributions — this crate is host-only.

## Necessity

- **Verdict:** Optional
- **Required for basic capabilities?** No. The basic workflow (open a forge, browse + edit markdown, search, git) does not invoke an LSP server. Markdown has no Tier-1 language server in this tree; the host is dormant when `lsp.toml` is absent.
- **Depended on by:** the shell diagnostics plugin (`shell/src/plugins/nexus/diagnostics/diagnosticsStore.ts`) subscribes to `com.nexus.lsp.textDocument.publishDiagnostics`; the editor's CM6 LSP integration consumes the position-request handlers. Both gracefully no-op when no servers are configured.
- **Depends on:** `nexus-kernel`, `nexus-plugins`. No service-plugin dependencies.
- **What breaks if removed:** the diagnostics panel goes empty; CM6 loses completions / hover / go-to-definition / rename / format / code actions for any code-editing surface a user opens. Markdown editing is unaffected. Plugin contributions of LSP servers (BL-113 Phase 2b) would have nowhere to land.

## Notes

- Active codepath today: present-but-dormant for a typical Nexus user (no `lsp.toml` in a markdown-only forge). Real consumers are the shell editor extensions and `crates/nexus-bootstrap/src/plugins/lsp.rs` contribution wiring. No CLI/TUI verbs.
- BL-113 Phase 2b register/unregister verbs have no verb-level capability gate today (ADR 0027 §Open Question #3 — hardening filed as a follow-up); spawn capability is checked at process spawn, not at register.
- Shutdown timeout hard-coded at 5s in `core_plugin.rs:331` (flagged in `docs/0.1.2/settings/hardcoded-rust.md`).
- Crate is well-tested: 18 tests in `core_plugin.rs` + extensive `config.rs` BL-113 coverage. Stable surface.
