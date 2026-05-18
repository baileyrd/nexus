# Glossary

| Term | Meaning |
|------|---------|
| **Forge** | A user-chosen directory of markdown files plus its `.forge/` index sidecar — the unit of "a Nexus workspace". |
| **Bootstrap** | `crates/nexus-bootstrap/`, the sole place in the workspace that links every service crate. Frontends consume `build_cli_runtime` / `build_tui_runtime` / `init_forge`. |
| **Invoker** | Anything that drives the kernel from outside: CLI, TUI, MCP server, Tauri shell. Each obtains a `Runtime` from the bootstrap and routes through `context.ipc_call(...)`. |
| **CorePlugin** | A native Rust plugin registered at boot; trait at `crates/nexus-plugins/src/loader.rs`. 23 of them ship in-tree. |
| **Community plugin** | A WASM or JS plugin loaded at runtime, capability-gated. Cannot impersonate a core plugin (rejected at load if `trust_level = "core"`). |
| **Handler id** | The single-dispatch integer that identifies an IPC verb on a plugin — see ADR 0005. Plugins register `(handler_id, command_string)` pairs at boot. |
| **Capability** | One of 30 string-named gate values checked at IPC dispatch. Examples: `fs.read`, `net.http`, `process.spawn`. See [`../capabilities.md`](../capabilities.md). |
| **cap_matrix** | `crates/nexus-bootstrap/cap_matrix.toml` — the authoritative `(plugin, handler) → caps` map. `cap_matrix_complete` test enforces completeness. |
| **Grant** | The user's install-time decision about which optional capabilities a community plugin gets. Persisted to `<plugin_dir>/granted_caps.json`, sealed with `chacha20poly1305`. |
| **Slot** | A named extension point in the shell (`slot:editor-area`, `slot:status-bar.left`, etc.) that plugins contribute UI to via `Registrations`. |
| **Leaf** | A pane that holds a single content type (editor, terminal, graph, etc.). The workspace is a tree of leaves with drag-to-split. See ADR 0011 §leaf model. |
| **Pre-0.1.2 archive** | `docs/archive/pre-0.1.2/` — every doc that was active prior to the v0.1.2 cut. Kept verbatim for cross-reference; replaced authoritatively by this `0.1.2/` directory. |
| **AUDIT flag** | A `# AUDIT:` comment in `cap_matrix.toml` marking a handler whose current cap classification is a candidate for elevation. See [`audit-flags.md`](audit-flags.md). |
| **Drift check** | `scripts/check_ipc_drift.sh` — regenerates `packages/nexus-extension-api/src/generated/ipc/`, `crates/nexus-bootstrap/schemas/ipc/`, and `docs/generated/capabilities.md`; fails CI if any file changed without being committed. |
| **ADR** | Architecture Decision Record. Numbered 0001–0029 in the archive (`docs/archive/pre-0.1.2/adr/`). |
| **PRD** | Product Requirements Document. Numbered 01–17 in the archive. |
| **IPC** | Inter-process call inside the kernel — `context.ipc_call(plugin_id, command, args) -> Result<serde_json::Value>`. This is the single mediated path between subsystems. |
| **WSLg** | Windows Subsystem for Linux's GUI layer. Requires `WEBKIT_DISABLE_*` + `GDK_BACKEND=x11` env vars to render the Tauri shell — baked into `shell/`'s `tauri:dev` script. |
