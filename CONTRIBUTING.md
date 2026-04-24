# Contributing to Nexus

Thanks for your interest in Nexus. This document covers the policies that
affect where new work lands. Implementation details live in the crate
READMEs and the planning documents in `docs/`.

## The architecture in one sentence

Nexus is a microkernel knowledge environment. A small kernel
(`nexus-kernel`) coordinates independent service plugins (storage, AI,
editor, terminal, etc.), and multiple frontends (CLI, TUI, MCP server,
desktop shell) consume those services through a single IPC contract:

```
context.ipc_call(plugin_id, command, args) -> Result<serde_json::Value>
```

Every frontend funnels through that one path. It is the API.

---

## Desktop shell — single target

The plugin-first shell at `shell/` + `shell/src-tauri/` (crate
`nexus-shell`) is the single Tauri desktop target, per
[ADR 0011](docs/adr/0011-adopt-plugin-first-shell.md). The legacy
tri-pane shell (`app/` + `crates/nexus-app`) was retired in v0.4.0 —
see [`docs/legacy-shell-retirement.md`](docs/legacy-shell-retirement.md).
History remains recoverable via git (`v0.1.0-legacy-shell` tag).

### What this means day-to-day

- Add new IPC handlers to the appropriate `nexus-*` service crate so
  they are reachable from CLI, TUI, MCP, and the desktop shell through
  `context.ipc_call(...)`.
- Add new UI as a plugin in `shell/src/plugins/nexus/<feature>/`. The
  shell itself starts empty — every visible element is a plugin
  contribution.
- Do **not** reintroduce bespoke `#[tauri::command]` handlers in
  `shell/src-tauri/`. The bridge is deliberately minimal (`init_forge`,
  `boot_kernel`, `kernel_invoke`, `kernel_subscribe`,
  `kernel_unsubscribe`, `kernel_is_booted`, `shutdown_kernel`);
  capability belongs in a service crate behind IPC.

---

## Where things live

- `crates/nexus-kernel/` — event bus, IPC dispatcher, capability system,
  plugin lifecycle. Small; keep it that way.
- `crates/nexus-storage/` — file-as-truth, SQLite index, Tantivy
  full-text search, file watcher. Owns the forge.
- `crates/nexus-<service>/` — service plugins (AI, agent, editor, git,
  linkpreview, mcp, skills, terminal, theme, workflow, etc.). Each is a
  `CorePlugin` registered by `nexus-bootstrap` in a deterministic order.
- `crates/nexus-bootstrap/` — the orchestrator. Assembles a `Runtime`
  (kernel + registered plugins + invoker context) for any frontend.
- `crates/nexus-cli/` / `crates/nexus-tui/` / `crates/nexus-mcp/` —
  frontends that consume `nexus-bootstrap::build_*_runtime(forge_root)`
  and route everything through `context.ipc_call(...)`.
- `shell/src-tauri/` (crate `nexus-shell`) — the active desktop Tauri
  host. Thin bridge (`init_forge`, `boot_kernel`, `kernel_invoke`,
  `kernel_subscribe`, `kernel_unsubscribe`, `kernel_is_booted`,
  `shutdown_kernel`) + a handful of shell-side conveniences.
- `shell/src/` — the active desktop frontend. `ExtensionHost` loads
  plugins from `shell/src/plugins/{core, nexus, community}/`. The shell
  starts **empty** — every visible UI element is a plugin contribution.
- `packages/nexus-extension-api/` — the stable TypeScript contract for
  shell plugin authors (`@nexus/extension-api`). Scaffolded; wire-in
  pending.

Architectural decisions live in `docs/adr/`. Feature plans live in
`docs/` (e.g. `leaf-architecture.md`, `shell-kernel-bridge-plan.md`,
`canvas-shell-plan.md`, `bases-shell-plan.md`).

---

## Guardrails

A workspace-level test (`crates/nexus-bootstrap/tests/dep_invariants.rs`)
fails the build if a frontend or service crate reaches around the IPC
boundary (for example, if `nexus-cli` tries to import `rusqlite`
directly). Do not disable or weaken that test. If you hit it, you're
trying to do something the architecture forbids for good reason — route
through IPC instead.

---

## Commits and PRs

- Small, focused commits. One logical change per commit.
- Reference the ADR or PRD section that motivates architectural changes.
- Commit bodies should explain the "why," not just the "what."
- Tag breaking changes to the plugin API surface. The deprecation policy
  is in [`DEPRECATED.md`](DEPRECATED.md): announce in one minor release,
  remove in the next.
