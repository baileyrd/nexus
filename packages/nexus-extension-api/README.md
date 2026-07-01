# @nexus/extension-api

TypeScript plugin contract for authoring Nexus plugins. See
[`CONTRACT_STATUS.md`](./CONTRACT_STATUS.md) for the contract history
and the per-tier surface inventory (#187).

## Install

```bash
npm install --save-dev @nexus/extension-api
```

## Status

`NexusPluginContext` is the **common plugin contract** — the subset of
verbs both in-tree runtimes hand plugins today. Each runtime's context
is a structural superset:

- the in-process shell host hands first-party plugins a `PluginAPI`
  (`shell/src/types/plugin.ts`);
- the iframe sandbox runtime hands community plugins a
  `SandboxedPluginContext` (`./src/sandbox/context.ts`).

Conformance is locked by compile-only tests
(`src/contractConformance.test-d.ts` here,
`shell/src/types/contractConformance.test-d.ts` shell-side) — a
contract edit either runtime cannot satisfy fails `tsc` in CI.

Where the tiers disagree on sync-versus-async, the contract's return
types are `MaybePromise<T>`; portable code should `await`
unconditionally (awaiting a plain value is a no-op).

## Portable usage (common contract)

Plugin *logic* can be typed against `NexusPluginContext` and handed
either runtime's context object:

```ts
import type { NexusPluginContext } from "@nexus/extension-api";

// Runs unchanged against either tier's context. Always `await`
// MaybePromise results.
export async function greet(ctx: NexusPluginContext): Promise<void> {
  const last = await ctx.storage.get("last-greeting");
  await ctx.storage.set("last-greeting", String(Date.now()));
  await ctx.notifications.show({
    message: last
      ? `Hello again from ${ctx.pluginId}!`
      : `Hello from ${ctx.pluginId}!`,
  });
}
```

Plugin *entry points* remain tier-specific: sandboxed plugins export a
`SandboxedPlugin` (`activate(ctx)` / `deactivate()`), first-party
plugins export the shell-side `Plugin` shape (`activate(api)` /
`deactivate()`). The four-hook `ScriptPlugin` shape is deprecated —
see [`DEPRECATED.md`](./DEPRECATED.md).

## What this package gives you

- **The common contract** (`NexusPluginContext`, `MaybePromise`,
  `NexusStatusBarItemHandle`) plus every contribution DTO
  (`EditorBlockType`, `Snippet`, `MenuItem`, `UriHandler`,
  `WebviewPanelConfig`, `TreeDataProvider`, `PanelNode`, …).
- **Sandbox-side runtime contracts** (`SandboxedPlugin`,
  `SandboxedPluginContext`) — implemented by the iframe sandbox
  runtime and safe to depend on.
- **ts-rs–generated Rust contract types** (`Capability`, `IpcError`,
  `PluginInfo`, `NexusEvent`, …) re-exported from `./generated`.

## What it does *not* give you

- The tier-specific surfaces beyond the common contract (`workspace`,
  `viewRegistry`, `configuration`, `editor` registration verbs, …) —
  those live on the runtime contracts; see the "tier-specific" table in
  [`CONTRACT_STATUS.md`](./CONTRACT_STATUS.md).
- React. Plugins that ship JSX must depend on `react` themselves; we
  intentionally avoid the dependency so the type package stays small.
- CodeMirror 6 extension types. Plugins that contribute decorations
  should `import type { Extension } from "@codemirror/state"` for
  sharper types; we export `EditorExtension` as an opaque alias so this
  package doesn't force a CM6 peer dependency.

## Versioning

Re-cut to `0.1.0` (repo-review V9 / #187): the original `1.0.0` tag
predated the runtime audit that surfaced the contract divergence and
implied a structural freeze that did not exist. `1.0.0` will be re-cut
once the common contract and its conformance gates have soaked for a
release. In-repo consumers are unaffected (`workspace:*`).

## Sandboxed Community Plugins

Community plugins run inside a null-origin iframe sandbox (WI-30). They
receive a `SandboxedPluginContext` — the common contract plus
declarative panels — because every method has to cross `postMessage`.
Key characteristics:

- Methods that are sync in the in-process tier are `async` here
  (`storage.*`, `notifications.show`, `context.*`,
  `statusBar.createItem`, …) — exactly the `MaybePromise` seams in the
  common contract.
- UI contributions are **declarative** — `views.registerPanel` takes a
  `render()` that returns a `PanelNode` tree. No React components
  (they can't be structured-cloned).
- No `workspace` / `viewRegistry` singletons, no service-plugin `fs`
  surface (use `ctx.platform.fs.*`), no `configuration` yet (use
  `ctx.storage` until the configuration bridge lands), no `internal`.

### Hello world

```ts
import { bootstrapSandboxedPlugin } from '@nexus/extension-api/sandbox/runtime';
import type { SandboxedPlugin } from '@nexus/extension-api';

const plugin: SandboxedPlugin = {
  async activate(ctx) {
    await ctx.notifications.show({
      message: 'Hello from the sandbox!',
      type: 'info',
    });

    ctx.commands.register('hello.greet', async () => {
      const name = await ctx.input.prompt('Your name?');
      if (name) await ctx.notifications.show({ message: `Hi ${name}!` });
    });

    ctx.views.registerPanel('hello.panel', () => ({
      type: 'vstack',
      gap: 8,
      children: [
        { type: 'heading', value: 'Hello', level: 2 },
        { type: 'button', label: 'Greet', commandId: 'hello.greet' },
      ],
    }));
  },

  deactivate() {
    // Host auto-sweeps subscriptions registered via `ctx.*.register`
    // and `ctx.*.on`. Free plugin-owned state (timers, buffers) here.
  },
};

bootstrapSandboxedPlugin(plugin);
```

### Runtime import path

`bootstrapSandboxedPlugin` is re-exported from the top-level barrel
(WI-30e) so a plugin source file needs a single import; the deep
`sandbox/runtime` path also works. The function is only *useful*
inside a sandboxed plugin bundle — the host never calls it, and
tree-shaking drops it from any host bundle that doesn't reference it.

Types (`SandboxedPlugin`, `SandboxedPluginContext`, `PanelNode`, …)
come from the root barrel and are safe to import anywhere.
