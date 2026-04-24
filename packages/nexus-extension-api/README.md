# @nexus/extension-api

Stable TypeScript types for authoring Nexus script plugins.

## Install

```bash
npm install --save-dev @nexus/extension-api
```

## Usage

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

- **Type definitions** for `NexusPluginContext` and every contribution DTO
  (`EditorBlockType`, `Snippet`, `MenuItem`, `UriHandler`,
  `WebviewPanelConfig`, `TreeDataProvider`, `PanelNode`, …).
- **The `ScriptPlugin` shape** your default export must satisfy.
- A **stable import surface** — the Nexus host already implements these
  shapes, so TypeScript will flag contract drift before you ship.

## What it does *not* give you

- A runtime. The `ctx` passed to your `dispatch` / lifecycle hooks is
  supplied by the Nexus host at load time. There is nothing to
  instantiate from this package.
- React. Plugins that ship JSX must depend on `react` themselves; we
  intentionally avoid the dependency so the type package stays small.
- CodeMirror 6 extension types. Plugins that contribute decorations
  should `import type { Extension } from "@codemirror/state"` for
  sharper types; we export `EditorExtension` as an opaque alias so this
  package doesn't force a CM6 peer dependency.

## Versioning

This package follows semver. A `1.x` tag means every exported shape is
frozen for the life of the major; new surfaces are additive. Breaking
changes land in a new major and are paired with a migration note in
`DEPRECATED.md` at the repo root.

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
