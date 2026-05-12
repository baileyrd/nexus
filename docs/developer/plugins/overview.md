# Plugins overview

A Nexus plugin is a unit of code that registers with the kernel,
contributes commands or UI, and reacts to events. This page covers
the **menu of options** before you commit to a specific shape.

## Tier choice: core vs. community

| | Core plugin | Community plugin |
|---|---|---|
| Language | Rust | TypeScript (compiled to WASM, or run in iframe) |
| Trust | Full host access | Capability-gated |
| Distribution | Compiled into the binary | Installed by the user from a `.wasm` / `.zip` |
| Hot reload | Requires rebuild | Yes, in-place |
| Crash isolation | None — a panic takes down Nexus | Sandbox-killable |
| Authoring | `impl CorePlugin for MyPlugin` in a workspace crate | Export `Plugin` from `index.ts`, drop in a forge |
| Best for | Built-in subsystems shipped with Nexus | Third-party extensions, theme tweaks, integrations |

If you're publishing for other Nexus users to install, write a
community plugin. If you're upstreaming a new built-in subsystem,
write a core plugin.

The rest of this page assumes **community plugin**. For core, see
[Core plugins / authoring](../core-plugins/authoring.md).

## Runtime choice: WASM vs. iframe-JS

Community plugins have two sandbox runtimes:

- **iframe-JS** (the `script` template) — your TypeScript runs in a
  null-origin `<iframe>`, communicating with the host over
  `postMessage`. Fast to iterate. Only available in the desktop
  shell. Best for UI-heavy plugins.
- **WASM** (the `wasm` template) — your TypeScript compiles to WASM
  via [Javy](https://github.com/bytecodealliance/javy) (or a Rust
  toolchain for performance-critical work). Runs everywhere — CLI,
  TUI, MCP server, shell. Best for plugins that should work outside
  the desktop shell.

The two runtimes use the same `@nexus/extension-api`. Most plugins
work unchanged on both; differences are noted in the API reference.

ADR: [`../../adr/0015-iframe-sandbox-plugin-runtime.md`](../../adr/0015-iframe-sandbox-plugin-runtime.md).

## What a plugin can contribute

| | Surface | Read |
|---|---|---|
| **Commands** | Palette entries with optional keybindings | [Commands and keybindings](../ui/commands-and-keybindings.md) |
| **Views** | Panels, sidebars, status-bar items | [Views and slots](../ui/views-and-slots.md) |
| **Editor extensions** | Decorations, slash commands, MDX components | [Editor / overview](../editor/overview.md) |
| **IPC handlers** | Callable from any other plugin or frontend | [IPC](ipc.md) |
| **Event subscribers** | React to file changes, AI completions, etc. | [Events](events.md) |
| **Settings** | Auto-rendered UI from a JSON schema | [Settings](settings.md) |
| **Themes** | CSS variable overrides | [Themes / build a theme](../themes/build-a-theme.md) |

A plugin can contribute any combination — there's no separate "theme
plugin" or "command plugin". The same manifest mechanism wires them
all up.

## What a plugin **can't** do

- **Bypass capabilities.** A plugin without `fs.write` cannot write
  to the forge, full stop. It can prompt the user to grant
  capabilities later, but the kernel won't honor an ungranted call.
- **Modify another plugin's data without permission.** IPC calls into
  another plugin require `ipc.call` capability and are subject to the
  target's own access checks.
- **Run code outside its sandbox.** The WASM or iframe boundary is
  hard. Plugins cannot reach the host process directly.
- **See other plugins' KV storage.** Each plugin has an isolated KV
  namespace.
- **Install other plugins.** Only the user (through the install flow)
  can grant capabilities and load plugins.

## Manifest

Every plugin has a `plugin.json`:

```json
{
  "id": "com.example.hello",
  "name": "Hello",
  "version": "1.0.0",
  "main": "index.js",
  "apiVersion": 1,
  "sandboxed": true,
  "capabilities": ["ui.notify", "kv.read", "kv.write"]
}
```

Full spec: [Manifest](manifest.md).

## Lifecycle

A plugin is `Loaded → Initialized → Running → Stopped` (or `Crashed`
on the error path). Activation is lazy: the runtime only loads
plugins when their declared activation events fire.

Full lifecycle: [Lifecycle](lifecycle.md).

## Where to look next

| If you're… | Read |
|---|---|
| Just trying to ship a useful plugin | [Getting started](../getting-started.md), then come back here |
| Wondering "can my plugin do X?" | [Capabilities reference](capabilities.md) |
| Trying to integrate with another plugin | [IPC](ipc.md) |
| Building UI | [Views and slots](../ui/views-and-slots.md) |
| Stuck on a runtime quirk | [`docs/shell/writing-a-plugin.md`](../../shell/writing-a-plugin.md) |
