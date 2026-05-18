# Legacy Shell Retirement — v0.4.0

**Status:** Retired. **Date:** 2026-04-24. **Phase:** 4, WI-37.

## What was retired

- `app/` — the React/Vite frontend for the legacy tri-pane Tauri desktop
  shell.
- `crates/nexus-app` — the Rust host crate (Tauri 2 binary) that hosted
  `app/` and exposed ~95 bespoke `#[tauri::command]` handlers.
- `crates/nexus-bootstrap/tests/legacy_freeze.rs` — the Phase-1 WI-22
  guardrail that capped the command count. No longer needed once the
  target crate is gone.

These are removed from the tree as of the commit named
`refactor: delete crates/nexus-app + app/ + legacy freeze test`.
History remains in git — `git log --all --full-history -- 'app/**' 'crates/nexus-app/**'`
recovers it.

## What replaces it

The plugin-first shell at `shell/` + `shell/src-tauri/` (crate
`nexus-shell`) is now the single desktop target per
[ADR 0011](adr/0011-adopt-plugin-first-shell.md). All UI surfaces are
plugins registered in `shell/src/plugins/{core,nexus,community}/`.
Backend capability lives in the `nexus-*` service crates and is reached
uniformly from every frontend (CLI, TUI, MCP, desktop) via
`context.ipc_call(plugin_id, command, args)`.

For anyone who had invoked `cargo run -p nexus-app` before: use
`cargo run -p nexus-cli` for the CLI, `cargo run -p nexus-tui` for the
TUI, or launch the desktop shell via the Tauri dev flow in `shell/`
(see `shell/README.md`).

## Forks and downstream consumers

If you were building on top of `crates/nexus-app` or `app/`:

- The `#[tauri::command]` handler set in `crates/nexus-app/src/lib.rs`
  was the legacy UI contract. Its capabilities now live as IPC commands
  on the service crates. `docs/archive/planning/SHELL-COMPARISON.md` (retained as a
  historical map) documents the per-command correspondence between the
  two shells.
- Persisted state migrates one-shot with
  `scripts/migrate-shell-state.ts`. Run it once against your legacy
  `layout-state.json` to produce a `shell-state.json` the new shell
  consumes. The script reads legacy file layout only — it does not
  depend on `nexus-app` at runtime, so it survives the deletion.
- UI extension points are now the stable `@nexus/extension-api`
  TypeScript contract (see `packages/nexus-extension-api/`). Port your
  behaviour, not your implementation — the shell primitives differ
  enough that a rewrite against `ExtensionHost` is lower friction than
  a porting layer.

## Why delete rather than keep as reference

- **No reverse deps.** Nothing inside the workspace depended on
  `nexus-app` before retirement (verified via
  `crates/nexus-bootstrap/tests/dep_invariants.rs`).
- **CI cost.** Keeping the crate in the workspace meant every
  `cargo build --workspace` compiled Tauri 2 + the legacy React build
  toolchain, for code no one ran.
- **Contributor confusion.** Two live shells meant every new feature
  had to answer "which one?" before starting. One shell, one answer.
- **Git preserves history.** Deletion is not erasure — the code is
  recoverable any time via the tag `v0.1.0-legacy-shell` or via
  `git log --all --full-history`.

## References

- [ADR 0011 — Adopt plugin-first shell](adr/0011-adopt-plugin-first-shell.md)
- `docs/archive/planning/SHELL-COMPARISON.md` — per-command legacy-vs-new map (historical)
- `docs/archive/planning/PHASE-4-IMPLEMENTATION-PLAN.md` §3.2 — the retirement plan
- `docs/archive/planning/PHASE-1-IMPLEMENTATION-PLAN.md` WI-22 — the freeze guardrail
  that preceded this retirement
