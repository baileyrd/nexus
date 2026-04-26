# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Common commands

Rust workspace (root `Cargo.toml`, excludes `shell/`):

```bash
cargo build --workspace                          # build all crates
cargo test  --workspace                          # run all tests
cargo clippy --workspace --all-targets           # lint
cargo test  -p <crate-name>                      # one crate's tests
cargo test  -p <crate-name> <test_name>          # one test by name substring
cargo build -p nexus-cli   # or -p nexus-tui     # build a single binary
```

pnpm workspace (`shell/` + `packages/*`, requires Node ≥18, pnpm ≥10):

```bash
pnpm install                                     # install workspace deps
pnpm --filter nexus-shell lint                   # eslint shell/src
pnpm --filter nexus-shell typecheck              # tsc --noEmit
pnpm --filter nexus-shell test                   # node --test tests/*.test.ts
pnpm --filter nexus-shell tauri:dev              # run desktop shell (needs webkit2gtk-4.1, libsoup-3.0)
```

IPC contract drift check — run before opening a PR that changes any IPC-typed struct:

```bash
scripts/check_ipc_drift.sh
```

It regenerates `packages/nexus-extension-api/src/generated/ipc/*.ts` (via ts-rs) and `crates/nexus-bootstrap/schemas/ipc/*.json` (via schemars) and fails if `git diff` is non-empty.

The other `scripts/test_*.sh` and `scripts/check_*.sh` helpers are thin wrappers around `cargo test -p <crate>` / `cargo check -p <crate>` with a hard-coded WSL path — prefer running cargo directly unless reproducing CI behaviour.

## Architecture

Nexus is a **microkernel** Rust workspace. Read `docs/ARCHITECTURE.md` and the ADRs under `docs/adr/` (especially 0004, 0005, 0011, 0016) before making structural changes.

**Four invariants drive the shape of the system:**

1. **File-as-truth.** Markdown files on disk are authoritative. The SQLite index in `.forge/index.db` and the Tantivy FTS index are rebuildable from the files; never write code that treats them as the source of record.
2. **Microkernel isolation.** `nexus-kernel` depends only on `nexus-types`. Subsystem crates depend on the kernel; the kernel never depends on a subsystem. This is enforced by `crates/nexus-bootstrap/tests/dep_invariants.rs` — if you hit it, the architecture is telling you to route through IPC instead.
3. **IPC over direct calls.** CLI, TUI, MCP server, and the Tauri shell all reach storage / AI / editor / etc. through one path:
   ```
   context.ipc_call(plugin_id, command, args) -> Result<serde_json::Value>
   ```
   Community WASM plugins use the same call. New capability ⇒ new IPC handler in the right service crate, not a new direct dependency from a frontend.
4. **Capabilities gate everything.** `fs.read`, `kv.write`, `ipc.call`, `events.publish`, etc. Every kernel-mediated operation checks a capability before it runs. See ADR 0002.

**Where things live:**

- `crates/nexus-kernel/` — event bus, IPC dispatcher, capability system, plugin lifecycle. Keep small.
- `crates/nexus-storage/` — file-as-truth, SQLite, Tantivy, file watcher, knowledge graph. Owns the forge.
- `crates/nexus-<service>/` — service plugins (`ai`, `agent`, `editor`, `git`, `linkpreview`, `mcp`, `skills`, `terminal`, `theme`, `workflow`, `database`, `kv`, `security`, `formats`, `panic-log`). Each is a `CorePlugin` registered by `nexus-bootstrap` in deterministic order.
- `crates/nexus-bootstrap/` — the orchestrator. Frontends call `build_cli_runtime(forge_root)` / `build_tui_runtime(forge_root)` to get a `Runtime` (kernel + registered plugins + invoker context).
- `crates/nexus-cli/` (`nexus`), `crates/nexus-tui/` (`nexus-tui`), `crates/nexus-mcp/` — frontends. They consume `nexus-bootstrap` and route through `context.ipc_call(...)`.
- `shell/` + `shell/src-tauri/` (crate `nexus-shell`) — the **single** active desktop target per ADR 0011. The legacy tri-pane `app/` + `crates/nexus-app` was retired in v0.4.0 (recoverable via `v0.1.0-legacy-shell` git tag).
- `shell/src/plugins/{core,nexus,community}/` — every visible UI element is a plugin contribution loaded by `ExtensionHost`. The shell starts empty.
- `packages/nexus-extension-api/` — the stable `@nexus/extension-api` TypeScript contract for shell plugin authors.

**Guardrails when adding features:**

- New backend capability ⇒ add an IPC handler to the appropriate `nexus-<service>` crate so it's reachable from CLI, TUI, MCP, and the shell uniformly. Do **not** add bespoke `#[tauri::command]` handlers in `shell/src-tauri/`; the bridge is intentionally minimal (`init_forge`, `boot_kernel`, `kernel_invoke`, `kernel_subscribe`, `kernel_unsubscribe`, `kernel_is_booted`, `shutdown_kernel`).
- New UI ⇒ add a plugin under `shell/src/plugins/nexus/<feature>/`, not a hard-coded shell component.
- If you change an IPC-boundary type (anything that flows through `ipc_call`), regenerate bindings with `scripts/check_ipc_drift.sh` before committing.

## Plugin tiers

- **Core plugins** — native Rust crates registered at bootstrap, full access.
- **Community plugins** — WASM-sandboxed via wasmtime, capability-gated. Scaffolded via `nexus plugin scaffold --type wasm ...`.

See `docs/writing-your-first-plugin.md` and `shell/docs/writing-a-plugin.md`.

## Forge layout

A "forge" is a user's directory of markdown files. Nexus stores its index alongside in `<forge>/.forge/`:

```
<forge>/
├── .forge/{index.db, search/, config.toml, logs/, temp/}
└── <user markdown files>
```

`NEXUS_FORGE_PATH` env var or `--forge-path` CLI flag selects the forge root.
