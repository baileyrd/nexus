# Architecture primer

You don't need to understand the kernel to build a plugin, but five
minutes here will save you fifty minutes of confused debugging later.

## The shape

```
┌──────────────────────────────────────────────────────┐
│  Frontends:  shell · CLI · TUI · MCP server          │
└─────────────────────┬────────────────────────────────┘
                      │ ipc_call(plugin_id, command, args)
┌─────────────────────▼────────────────────────────────┐
│  nexus-kernel:  event bus · IPC dispatcher ·         │
│                 capability gate · plugin lifecycle   │
└─────┬──────────┬───────────┬───────────┬─────────────┘
      │          │           │           │
┌─────▼────┐ ┌──▼─────┐ ┌───▼────┐ ┌────▼─────┐
│ storage  │ │   ai   │ │ editor │ │ …other   │
└──────────┘ └────────┘ └────────┘ └──────────┘
   core plugins (native Rust, full access)

      ┌──────────────────────────────┐
      │  community plugins (sandbox) │
      │  WASM or iframe-JS · capability-gated
      └──────────────────────────────┘
```

The kernel is small and stable. Everything user-visible — file
storage, AI, editor, terminal, search panel, themes — is a plugin
calling other plugins through one path: `context.ipc_call()`.

Authoritative diagrams: [`../architecture/C4.md`](../architecture/C4.md).

## Four invariants

These shape what's possible and what isn't:

1. **File-as-truth.** Markdown on disk is authoritative. Indexes are
   derived. Don't write code that treats the SQLite index as a
   source of record — the watcher will overwrite you.

2. **Microkernel isolation.** `nexus-kernel` depends only on
   `nexus-types`. Subsystem crates depend on the kernel; the kernel
   never depends on a subsystem. Enforced by
   `crates/nexus-bootstrap/tests/dep_invariants.rs`. For plugin
   authors this means: **never** assume the kernel knows about your
   plugin's domain. Route requests through IPC.

3. **IPC over direct calls.** All four frontends and every plugin
   reach storage / AI / editor / etc. through one path:
   ```
   context.ipc_call(plugin_id, command, args) -> Result<Json>
   ```
   When you contribute a new capability, expose it as an IPC handler
   so it's reachable from CLI, TUI, MCP, and shell uniformly.

4. **Capabilities gate everything.** Every kernel-mediated operation
   checks a capability before it runs. See
   [Capabilities reference](plugins/capabilities.md) for the full
   list.

Long-form: [`../architecture/invariants.md`](../architecture/invariants.md).

## Two trust tiers

| Tier | Runtime | Trust | Author |
|---|---|---|---|
| **Core** | Native Rust crate, in-process | Full host access | Nexus team or blessed |
| **Community** | WASM or iframe-JS sandbox | Capability-gated | Anyone |

Both register through the same plugin lifecycle and contribute through
the same APIs. The shape of the code differs (Rust trait
implementation vs. TypeScript module export), the trust model
differs, but the user-facing surface is identical.

The choice is usually obvious: if you're writing third-party code,
build a community plugin. If you're contributing a new built-in
subsystem to upstream Nexus, write a core plugin. See
[Plugins / overview](plugins/overview.md) for the trade-offs.

ADR: [`../adr/0016-microkernel-native-vs-wasm-plugin-split.md`](../adr/0016-microkernel-native-vs-wasm-plugin-split.md).

## What lives where

| Crate | Role |
|---|---|
| `nexus-kernel` | Event bus, IPC dispatcher, capability checker, plugin lifecycle |
| `nexus-types` | Stable types shared everywhere |
| `nexus-plugin-api` | Public Rust API for plugin authors |
| `nexus-plugins` | Plugin loader (WASM + sandbox) |
| `nexus-bootstrap` | Wires the kernel + every core plugin into a `Runtime` |
| `nexus-storage`, `nexus-ai`, `nexus-editor`, … | Core plugins (one per subsystem) |
| `nexus-cli`, `nexus-tui`, `nexus-mcp` | Frontends |
| `shell/` + `shell/src-tauri/` | Tauri desktop shell (TypeScript + Rust bridge) |
| `packages/nexus-extension-api/` | The TypeScript `@nexus/extension-api` package |

If you're writing a community plugin, you mostly interact with
`@nexus/extension-api`. If you're writing a core plugin, you implement
a `CorePlugin` trait from `nexus-plugin-api` and register in
`nexus-bootstrap`.

## What changes vs. what's stable

- **Stable**: the kernel public API, the capability vocabulary, the
  plugin lifecycle hooks, the IPC dispatch contract. Breaking changes
  here move the API version (the `apiVersion` field in the manifest).
- **Evolving**: which IPC commands each core plugin exposes. New
  commands are added; existing ones rarely change shape (and when they
  do, the IPC drift check catches it).
- **Internal**: bootstrap order, the leaf-tree implementation,
  workspace.json schema. You shouldn't need to touch these.

## Read next

Pick the page that matches what you're building. Most authors start at
[Plugins / overview](plugins/overview.md).
