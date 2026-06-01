# @nexus/extension-api

Forward-looking TypeScript types for authoring Nexus plugins. See
[`CONTRACT_STATUS.md`](./CONTRACT_STATUS.md) for the current runtime
divergence vs. the headline `NexusPluginContext` / `ScriptPlugin`
shapes (#187).

## Install

```bash
npm install --save-dev @nexus/extension-api
```

## Status

The `NexusPluginContext` / `ScriptPlugin` shapes this package exports
are the *target* contract — they are **not** what any current Nexus
runtime supplies. Today the in-process shell host hands plugins a
`PluginAPI` (`shell/src/types/plugin.ts`) and the sandbox runtime hands
plugins a `SandboxedPluginContext` (`./src/sandbox/context.ts`); both
overlap with the target shape on the verbs that matter but diverge on
field names and on the sync-versus-async boundary. See
[`CONTRACT_STATUS.md`](./CONTRACT_STATUS.md) for the per-field
inventory.

Until reconciliation lands, plugin authors should import the
runtime-specific context types directly. The `NexusPluginContext`
example in the next section compiles fine but the resulting plugin
will not load — `loadScriptPlugin` is not implemented in either
runtime.

## Aspirational usage (target contract, #187)

```ts
import type { ScriptPlugin, NexusPluginContext } from "@nexus/extension-api";

const plugin: ScriptPlugin = {
  async onInit(ctx) {
    ctx.disposables.add(
      ctx.editor.registerSnippet({
        id: "plugin:com.example.todo:insert",
        trigger: "td",
        body: "- [ ] $CURSOR",
      }),
    );
  },

  async dispatch(_handlerId, _args, ctx) {
    ctx.ui.notify("info", "hello from com.example.todo");
  },
};

export default plugin;
```

## What this package gives you

- **Forward-looking type definitions** for `NexusPluginContext` and
  every contribution DTO (`EditorBlockType`, `Snippet`, `MenuItem`,
  `UriHandler`, `WebviewPanelConfig`, `TreeDataProvider`, `PanelNode`,
  …).
- **The aspirational `ScriptPlugin` shape** a plugin's default export
  will eventually satisfy.
- **Sandbox-side runtime contracts** (`SandboxedPlugin`,
  `SandboxedPluginContext`) — these *are* implemented today by the
  iframe sandbox runtime and are safe to depend on.

## What it does *not* give you

- A runtime for the headline `NexusPluginContext` / `ScriptPlugin`
  shapes. Neither the in-process shell nor the sandbox runtime
  implements them; see Status above.
- React. Plugins that ship JSX must depend on `react` themselves; we
  intentionally avoid the dependency so the type package stays small.
- CodeMirror 6 extension types. Plugins that contribute decorations
  should `import type { Extension } from "@codemirror/state"` for
  sharper types; we export `EditorExtension` as an opaque alias so this
  package doesn't force a CM6 peer dependency.

## Versioning

The current `1.0.0` tag predates the runtime audit that surfaced the
divergence above; it does **not** yet imply structural freeze. A
future release will cut the line cleanly once one runtime shape is
canonical and the others adopt it. Until then, treat the headline
shapes as forward-looking documentation rather than as a stability
guarantee. The sandbox-side shapes (`SandboxedPlugin`,
`SandboxedPluginContext`) are tracked separately and *are* the live
contract for sandboxed plugins.

## Sandboxed Community Plugins

Community plugins run inside a null-origin iframe sandbox (WI-30). They
import a **different** context shape — `SandboxedPluginContext` —
because every method has to cross `postMessage`. Key differences from
first-party `NexusPluginContext`:

- Several sync methods become `async` (`storage.*`, `notifications.show`,
  `context.*`, `statusBar.createItem`, …).
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

`bootstrapSandboxedPlugin` lives in the sandbox runtime module and is
**not** re-exported from the top-level barrel — it's only useful
inside a sandboxed plugin bundle (it runs top-level side effects on
import). Bring it in directly:

```ts
import { bootstrapSandboxedPlugin } from '@nexus/extension-api/sandbox/runtime';
import type { SandboxedPlugin } from '@nexus/extension-api';
```

Types (`SandboxedPlugin`, `SandboxedPluginContext`, `PanelNode`, …)
come from the root barrel and are safe to import anywhere.
