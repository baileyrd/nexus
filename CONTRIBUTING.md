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

## Desktop shell freeze — 2026-04-23

The repo currently carries two Tauri desktop shells:

| Directory / crate | Role | Status |
|---|---|---|
| `shell/` + `shell/src-tauri/` (crate `nexus-shell`) | **Active**. Plugin-first, VS-Code-style. 32 `nexus.*` plugins registered. | All new desktop work lands here. |
| `app/` + `crates/nexus-app` | **Deprecated**. Legacy tri-pane layout, ~95 bespoke Tauri commands. | Frozen. Bug fixes and security patches only. |

Per [ADR 0011](docs/adr/0011-adopt-plugin-first-shell.md) (Accepted
2026-04-23), the plugin-first shell is the single desktop target. The
legacy shell is preserved only until feature parity is reached, then
deleted.

### What this means day-to-day

**Do:**

- Add new IPC handlers to the appropriate `nexus-*` service crate so
  they are reachable from CLI, TUI, MCP, and the new shell through
  `context.ipc_call(...)`.
- Add new UI as a plugin in `shell/src/plugins/nexus/<feature>/`.
- Fix bugs in `app/` + `crates/nexus-app` if they're blocking users
  today. Mark the fix `legacy-only` in the commit message.

**Do not:**

- Add new `#[tauri::command]` handlers to `crates/nexus-app/src/*.rs`.
  If you think you need one, the same capability needs to exist in the
  new shell, and the right answer is a service-crate IPC handler, not a
  Tauri command.
- Add new React components, stores, or plugin extension points in
  `app/src/`. Those go into `shell/src/` instead.
- Copy-paste `app/` code into `shell/`. Port the *behaviour*, not the
  implementation — the architectures are different enough that rewriting
  against the new shell's primitives is the right move.

### How to know you're in scope

Before touching `app/` or `crates/nexus-app`, ask:

1. Is this a bug fix for something shipping users depend on today?
2. Is this a security patch that can't wait for new-shell parity?

If neither, open an issue describing the capability and it'll be
scheduled against the parity checklist
([`docs/Shell-Capability-Comparison.xlsx`](docs/Shell-Capability-Comparison.xlsx)).

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
- `app/` + `crates/nexus-app/` — **deprecated legacy shell**. See the
  freeze policy above.

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
