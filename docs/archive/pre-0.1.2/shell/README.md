# Shell Documentation

> [!WARNING]
> **STALE — see [DG-02](../roadmap/DOC-GAPS.md#dg-02--docsshell-reference-is-post-leaf-migration-stale).**
> Several concrete claims in this tree are wrong post-leaf-migration:
> documented slot count (8) vs. real (6); plugin count (38) vs. real
> (60); namespace (`core.*`) vs. real (`nexus.*`); workspace path
> (`.nexus/workspace.json`) vs. real (`<forge>/.forge/workspace.json`);
> `PluginAPI` doc covers ~10 of ~17 sub-surfaces; multiple registry
> and contribution shapes are fabricated. Cross-check against the
> source under [`../../shell/src/`](../../shell/src/) and
> [`../../packages/nexus-extension-api/src/`](../../packages/nexus-extension-api/src/)
> before relying on a documented interface.

Documentation specific to the Nexus desktop shell — the Tauri 2 + Vite +
React app at `shell/` (crate `nexus-shell`). For repository-wide docs,
see [`../../docs/README.md`](../../docs/README.md).

## I'm writing a shell plugin

| Read | What it gives you |
|---|---|
| [`writing-a-plugin.md`](writing-a-plugin.md) | The reference: manifest, sandbox, capabilities, slot system, worked example |
| [`../../docs/plugin-authors/quickstart.md`](../../docs/plugin-authors/quickstart.md) | Scaffold and run your first plugin (start here if you haven't booted one yet) |
| [`plugin-api.md`](plugin-api.md) | The `@nexus/extension-api` import surface — every method/contribution type |
| [`plugin-system.md`](plugin-system.md) | Manifest shape, `activate()`/`deactivate()`, core vs community distinction |
| [`extension-host.md`](extension-host.md) | Host load order, dependency resolution, lifecycle states |

## I'm modifying the shell internals

| Read | What it gives you |
|---|---|
| [`architecture.md`](architecture.md) | Four-layer substrate. The contract: shell never hardcodes UI |
| [`registry-system.md`](registry-system.md) | PluginRegistry, CommandRegistry, SlotRegistry (Zustand), ViewRegistry, etc. |
| [`slot-system.md`](slot-system.md) | The 8 slot IDs, priority ordering, "empty shell proof" |
| [`event-bus.md`](event-bus.md) | Synchronous EventBus, naming conventions, cleanup |
| [`context-keys.md`](context-keys.md) | when-clause expressions, ContextKeyService, built-in keys |
| [`workspace-layout.md`](workspace-layout.md) | `workspace.json` per forge, leaf union, chrome vs content |
| [`core-plugins.md`](core-plugins.md) | Load order, default-on/off catalog, the 38 built-ins |

## Reference (not authoritative for Nexus)

| Read | What it gives you |
|---|---|
| [`obsidian/obsidian-runtime.md`](obsidian/obsidian-runtime.md) | Reverse-engineered Obsidian `app.js`: workspace model, view lifecycle, plugin API surface |
| [`obsidian/obsidian-measurements.md`](obsidian/obsidian-measurements.md) | Obsidian CSS measurements (token cascade, chrome, ribbon, sidebar) |

These are reference notes captured while porting Workspace/Leaf semantics.
They describe Obsidian's behaviour for parity work. They are **not**
authoritative for Nexus implementation — when Obsidian's behaviour and a
Nexus design choice diverge, the Nexus design wins.

## Archive

Historical shell docs live at [`archive/`](archive/) — see
[`archive/README.md`](archive/README.md) for the inventory and the convention.
