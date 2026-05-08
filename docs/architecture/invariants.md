# Architectural Invariants

The four rules that drive the shape of Nexus. These should hold across every
subsystem; if a change appears to require violating one of them, the architecture
is telling you to route the work differently. Each is enforced or codified
somewhere — the enforcement mechanism is named in each section.

## 1. File-as-truth

Markdown files on disk are authoritative. The SQLite index in `.forge/index.db`
and the Tantivy FTS index under `.forge/search/` are *rebuildable* from those
files. Never write code that treats the index as the source of record — if a
file changes outside Nexus, the watcher (in `nexus-storage`) brings the index
back into agreement; if the index disappears, the storage layer rebuilds it
on next start.

**Enforced by:** the storage subsystem owns the file watcher
([ADR 0003](../adr/0003-storage-owns-file-watcher.md)) and the index is
explicitly designed as derived state.

## 2. Microkernel isolation

`nexus-kernel` depends only on `nexus-types` and `nexus-plugin-api` (both are leaf crates with no internal `nexus-*` dependencies). Subsystem crates depend on the
kernel; the kernel never depends on a subsystem. Frontends and service crates
must not reach around the IPC boundary by linking subsystems directly (e.g.,
`nexus-cli` cannot import `rusqlite`).

**Enforced by:** `crates/nexus-bootstrap/tests/dep_invariants.rs` parses the
`Cargo.toml` of each frontend / IPC consumer crate and fails if a forbidden
direct dependency is present. If you hit it, route through IPC instead.

## 3. IPC over direct calls

The CLI, TUI, MCP server, and Tauri desktop shell all reach storage / AI /
editor / etc. through one path:

```
context.ipc_call(plugin_id, command, args) -> Result<serde_json::Value>
```

Community WASM plugins use the same call. New backend capability ⇒ new IPC
handler in the right service crate, not a new direct dependency from a
frontend. The Tauri bridge in `shell/src-tauri` is intentionally thin: real
capability flows through `kernel_invoke` → `ipc_call`, not through bespoke
`#[tauri::command]` handlers.

**Enforced by:** the same dep-invariants test as (2), plus
[ADR 0005](../adr/0005-single-dispatch-handler-ids.md) which fixes the
single-dispatch handler-ID convention.

## 4. Capabilities gate everything

`fs.read`, `fs.write`, `kv.read`, `kv.write`, `ipc.call`, `events.publish`,
`net.fetch`, `exec.spawn`, etc. Every kernel-mediated operation checks a
capability before it runs. A plugin's manifest declares required and optional
capabilities; the kernel grants and audits each call.

**Enforced by:** `nexus-security` audit logging and the capability checks in
`nexus-kernel`'s IPC dispatcher. Capability taxonomy is hierarchical and
documented in [ADR 0002](../adr/0002-hierarchical-capability-strings.md).

---

## Why these and not others

These four are the rules whose violation has historically caused the worst
problems. Other useful patterns — deterministic plugin load order, KV-backed
plugin state, closed event enums, etc. — are individually documented as ADRs
but are recoverable when violated. Breaking any of the four above tends to
require a multi-week unwind.
