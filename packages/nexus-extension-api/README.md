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
