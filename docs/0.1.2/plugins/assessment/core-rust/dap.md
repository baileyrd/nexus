# com.nexus.dap

- **Path:** `crates/nexus-dap/`
- **Tier:** Core Rust
- **Bootstrap order:** 20

## Architecture

- Entry point `crates/nexus-dap/src/lib.rs` re-exports `DapCorePlugin`, `DapHostConfig`, `ConnectionPool`, and the `protocol` module's `ProtocolMessage` / `ProtocolRequest` / `ProtocolResponse` / `ProtocolEvent`. Registered by `crates/nexus-bootstrap/src/plugins/dap.rs` with all three lifecycle hooks enabled.
- Key modules: `core_plugin` (21 handlers), `config` (TOML loader + BL-113 contribution map), `client` (stdio adapter wrapper, `AdapterCapabilities`, `SourceBreakpointSpec`), `pool` (per-adapter lazy `ConnectionPool` with reconnect-with-backoff), `protocol` (DAP `type`-tagged envelope, `seq` correlation ids), `transport` (Content-Length JSON framing).
- `on_init` reads `<forge>/.forge/dap.toml`; missing file yields empty config, parse errors warn + disable. `on_start` publishes `com.nexus.dap.started` with `configured_adapters` count. `on_stop` mirrors LSP — 5s hard-capped pool shutdown.
- Persistence: `<forge>/.forge/dap.toml` (forge-config.md:110). No SQLite. Breakpoints are remembered per-`LspClient`-equivalent in memory via `client.remember_breakpoints` so reconnect can replay them.
- Settings owned: the `[[adapters]]` array in `dap.toml` (`crates/nexus-dap/src/config.rs`). No `[dap]` top-level block in `app.toml`. BL-113 contributions land in `DapHostConfig::contributed_by`; wiring lives in `crates/nexus-bootstrap/src/dap_contribution_wiring.rs`.
- External dependencies: spawns arbitrary child processes (configured `command`), reads/writes stdio. No network from this crate; adapters themselves attach to debuggees over whatever transport they choose. Pulls `tokio`, `toml`, `tracing`.

## Surface

- IPC handlers (from `IPC_HANDLERS` in `core_plugin.rs:81`, 21 total):
  - `list_adapters` (1) — sync (registry shape) / also async variant for `connected` column.
  - `launch` (2), `attach` (3), `configuration_done` (4), `disconnect` (5), `terminate` (6) — session lifecycle.
  - `set_breakpoints` (7), `set_function_breakpoints` (8), `set_exception_breakpoints` (9).
  - `continue` (10), `next` (11), `step_in` (12), `step_out` (13), `pause` (14).
  - `threads` (15), `stack_trace` (16), `scopes` (17), `variables` (18), `evaluate` (19).
  - `register_adapter` (20), `unregister_adapter` (21) — BL-113 Phase 1b contributions.
- Bus events: adapter events fan out as `com.nexus.dap.<event>` (e.g. `initialized`, `stopped`, `continued`, `exited`, `terminated`, `thread`, `output`, `breakpoint`, `module`, `process`, `capabilities`). Body preserved verbatim.

## Necessity

- **Verdict:** Optional
- **Required for basic capabilities?** No. Debugging is orthogonal to markdown edit / search / git. The host is dormant without a `dap.toml`.
- **Depended on by:** shell debugger plugin (`shell/src/plugins/nexus/debugger/`) — wraps the IPC verbs and renders launch / breakpoints / threads / variables. First-party adapter scaffold lives at `plugins/first-party-dap-python/`.
- **Depends on:** `nexus-kernel`, `nexus-plugins`. No service-plugin dependencies.
- **What breaks if removed:** the shell Debugger panel cannot start sessions, set breakpoints, or step. BL-113 contributed DAP adapters have nowhere to register. No markdown / search / git workflow is affected.

## Notes

- Real consumers today: the shell debugger plugin (UI shell) + bootstrap contribution wiring + the `first-party-dap-python` example adapter. No CLI verbs.
- Architecture mirrors `nexus-lsp` 1:1 (same framing, same pool, same shutdown shape); differences contained in `protocol.rs`.
- BL-113 verb-level capability gate is missing (same ADR 0027 §OQ#3 caveat as LSP). Spawn capability checked at `launch` / `attach`, not at `register_adapter`.
- Hard-coded 5s shutdown deadline (flagged at `docs/0.1.2/settings/hardcoded-rust.md`).
- Generous test coverage (~25 unit tests in `core_plugin.rs` including BL-113 round-trips).
