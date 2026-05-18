# Plugins overview

Almost everything you see in Nexus is a plugin. The kernel itself only
provides the event bus, IPC dispatcher, capability system, and plugin
lifecycle. Editor, AI, terminal, search panel, file tree, themes, even
the activity bar — all plugins.

## Two tiers

**Core plugins** are native Rust crates compiled into the Nexus
binaries. They have full access to the host. The set is fixed at build
time. Examples: `com.nexus.storage`, `com.nexus.editor`,
`com.nexus.ai`, `com.nexus.terminal`, `com.nexus.git`,
`com.nexus.skills`, `com.nexus.workflow`, `com.nexus.agent`,
`com.nexus.theme`, `com.nexus.comments`, `com.nexus.canvas`,
`com.nexus.bases`, `com.nexus.kv`, `com.nexus.security`, etc.

**Community plugins** are sandboxed user-installed plugins, written in
TypeScript and run in a wasmtime WASM sandbox. They reach the host
through capability-gated IPC: `fs.read`, `kv.write`, `ipc.call`,
`events.publish`, etc. You grant capabilities at install time.

Both tiers use the same APIs. A community plugin and a core plugin
contributing the same view are indistinguishable to the user.

## What a plugin can contribute

- Commands (palette, keybindings)
- Views (panels, side bars, status-bar items)
- Editor extensions (decorations, slash commands, MDX components)
- Settings UI (auto-generated from a schema)
- Event handlers (file changed, note opened, etc.)
- IPC handlers (callable from any other plugin or frontend)

## Activation

Plugins lazy-load. They declare **activation events**:

- `onStartup` — load when Nexus starts
- `onCommand:<id>` — load when a specific command runs
- `onView:<id>` — load when a view is opened
- `onLanguage:markdown` — load when a markdown file is opened
- `onEvent:<topic>` — load when a topic publishes

An idle Nexus loads almost nothing — most plugins activate only when
you actually need them.

## Capability gates

Every kernel-mediated operation checks a capability. A plugin manifest
declares the capabilities it needs:

```json
{
  "id": "com.example.demo",
  "capabilities": ["fs.read", "ipc.call:com.nexus.storage/*"]
}
```

On install, you see the list and approve or deny. Denied capabilities
mean the plugin still loads but kernel calls return an error — well-
written plugins degrade gracefully. See
[ADR 0002](../../adr/0002-capability-system.md) for the design.

## Safe mode

Boot Nexus without community plugins:

```bash
nexus --safe-mode desktop
NEXUS_SAFE_MODE=1 nexus desktop
```

Useful for isolating whether a third-party plugin is causing a
problem. Core plugins always load.

## Manage installed plugins

```bash
nexus plugin list
nexus plugin install ./my-plugin.wasm
nexus plugin uninstall com.example.demo
nexus plugin enable com.example.demo
nexus plugin disable com.example.demo
nexus plugin settings com.example.demo
```

Or use the **Plugins** panel in the shell.

## Hot reload

Modify a plugin's `.wasm` and reinstall it; Nexus reloads it without a
restart. The dispatcher handles in-flight calls cleanly.
