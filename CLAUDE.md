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

Nexus is a **microkernel** Rust workspace. Read `docs/architecture/C4.md`, `docs/architecture/invariants.md`, and the ADRs under `docs/adr/` (especially 0004, 0005, 0011, 0016) before making structural changes. The doc index is at `docs/README.md`; for "what's actually shipped right now" see `docs/PRDs/IMPLEMENTATION_STATUS.md`.

**Four invariants drive the shape of the system:**

1. **File-as-truth.** Markdown files on disk are authoritative. The SQLite index in `.forge/index.db` and the Tantivy FTS index are rebuildable from the files; never write code that treats them as the source of record.
2. **Microkernel isolation.** `nexus-kernel` depends only on `nexus-types` and `nexus-plugin-api` (both leaf crates). Subsystem crates depend on the kernel; the kernel never depends on a subsystem. This is enforced by `crates/nexus-bootstrap/tests/dep_invariants.rs` — if you hit it, the architecture is telling you to route through IPC instead.
3. **IPC over direct calls.** CLI, TUI, MCP server, and the Tauri shell all reach storage / AI / editor / etc. through one path:
   ```
   context.ipc_call(plugin_id, command, args) -> Result<serde_json::Value>
   ```
   Community WASM plugins use the same call. New capability ⇒ new IPC handler in the right service crate, not a new direct dependency from a frontend.
4. **Capabilities gate everything.** `fs.read`, `kv.write`, `ipc.call`, `events.publish`, etc. Every kernel-mediated operation checks a capability before it runs. See ADR 0002.

**Where things live:**

- `crates/nexus-kernel/` — event bus, IPC dispatcher, capability system, plugin lifecycle. Keep small.
- `crates/nexus-storage/` — file-as-truth, SQLite, Tantivy, file watcher, knowledge graph. Owns the forge.
- `crates/nexus-<service>/` — service plugins (`ai`, `agent`, `comments`, `editor`, `git`, `linkpreview`, `mcp`, `skills`, `templates`, `terminal`, `theme`, `workflow`, `database`, `kv`, `security`, `formats`, `panic-log`). Each is a `CorePlugin` registered by `nexus-bootstrap` in deterministic order. The full Cargo workspace has 25 members; see `Cargo.toml` for the authoritative list (also includes `nexus-types`, `nexus-plugin-api`, and `nexus-plugins` which are libraries rather than service plugins).
- `crates/nexus-bootstrap/` — the orchestrator. Frontends call `build_cli_runtime(forge_root)` / `build_tui_runtime(forge_root)` to get a `Runtime` (kernel + registered plugins + invoker context).
- `crates/nexus-cli/` (`nexus`), `crates/nexus-tui/` (`nexus-tui`), `crates/nexus-mcp/` — frontends. They consume `nexus-bootstrap` and route through `context.ipc_call(...)`.
- `shell/` + `shell/src-tauri/` (crate `nexus-shell`) — the **single** active desktop target per ADR 0011. The legacy tri-pane `app/` + `crates/nexus-app` was retired in v0.4.0 (recoverable via `v0.1.0-legacy-shell` git tag).
- `shell/src/plugins/{core,nexus,community}/` — every visible UI element is a plugin contribution loaded by `ExtensionHost`. The shell starts empty.
- `packages/nexus-extension-api/` — the stable `@nexus/extension-api` TypeScript contract for shell plugin authors.

**Guardrails when adding features:**

- New backend capability ⇒ add an IPC handler to the appropriate `nexus-<service>` crate so it's reachable from CLI, TUI, MCP, and the shell uniformly. Do **not** add bespoke `#[tauri::command]` handlers in `shell/src-tauri/` for new capability; route it through `kernel_invoke` → `ipc_call` instead. The Tauri bridge in `shell/src-tauri/src/lib.rs` registers 23 commands today, grouped by intent: 7 kernel (`init_forge`, `boot_kernel`, `kernel_invoke`, `kernel_subscribe`, `kernel_unsubscribe`, `kernel_is_booted`, `shutdown_kernel`), 5 plugin-management (`scan_plugin_directory`, `scan_plugin_directory_at`, `set_plugin_enabled`, `get_plugin_granted_capabilities`, `set_plugin_granted_capabilities`), 4 persistence (`get_shell_state`, `save_shell_state`, `write_last_forge_path`, `forget_forge_path`), 2 utility (`path_exists`, `append_shell_log`), and 5 popout (`popout_window`, `close_popout_window`, `list_popout_windows`, `get_popout_window_bounds`, `set_popout_window_bounds`) per [ADR 0020](docs/adr/0020-popout-window-architecture.md). Adding a new shell-management command (popout, persistence, etc.) is OK if it's intrinsic to the host; adding a new feature command is not.
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

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **nexus** (32417 symbols, 58623 relationships, 300 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> If any GitNexus tool warns the index is stale, run `npx gitnexus analyze` in terminal first.

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `gitnexus_impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `gitnexus_detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `gitnexus_query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `gitnexus_context({name: "symbolName"})`.

## Never Do

- NEVER edit a function, class, or method without first running `gitnexus_impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `gitnexus_rename` which understands the call graph.
- NEVER commit changes without running `gitnexus_detect_changes()` to check affected scope.

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/nexus/context` | Codebase overview, check index freshness |
| `gitnexus://repo/nexus/clusters` | All functional areas |
| `gitnexus://repo/nexus/processes` | All execution flows |
| `gitnexus://repo/nexus/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->
