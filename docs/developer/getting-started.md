# Getting started

This page gets you from zero to a working plugin you can run inside
Nexus in about ten minutes. For the in-depth version of every step,
follow the links to the reference pages.

## Prerequisites

- A working Nexus build — see
  [`../help/getting-started/install.md`](../help/getting-started/install.md).
- Node ≥ 18 and pnpm ≥ 10.
- A forge to install into (`nexus forge init ~/scratch`).

## 1. Scaffold

```bash
nexus plugin scaffold \
  --template script \
  --id com.example.hello \
  --name "Hello"
cd hello
```

You'll get:

```
hello/
├── plugin.json          # manifest
├── src/index.ts         # entry point
├── package.json
└── tsconfig.json
```

The `script` template targets the iframe-JS sandbox (the easiest
runtime for getting started). Other templates: `wasm` (WASM sandbox),
`theme` (CSS-only).

## 2. Look at the manifest

```json
{
  "id": "com.example.hello",
  "name": "Hello",
  "version": "1.0.0",
  "main": "index.js",
  "apiVersion": 1,
  "sandboxed": true,
  "capabilities": ["ui.notify"]
}
```

The full manifest spec lives at
[`plugins/manifest.md`](plugins/manifest.md). For now, three fields
matter: `id` (reverse-DNS, must be unique), `main` (entry point), and
`capabilities` (what kernel operations you can call).

## 3. Look at the entry point

```ts
import type { Plugin, PluginContext } from '@nexus/extension-api';

export const plugin: Plugin = {
  id: 'com.example.hello',
  activate(ctx: PluginContext) {
    ctx.commands.register({
      id: 'hello.sayHi',
      title: 'Hello: Say Hi',
      run: async () => {
        const name = await ctx.ui.prompt({ message: 'Your name?' });
        ctx.ui.notify({ message: `Hello, ${name}!` });
      },
    });
  },
  deactivate() {
    /* clean up */
  },
};
```

Two lifecycle hooks: `activate` (called once when the plugin is
loaded) and `deactivate` (called on unload or shutdown). The
`PluginContext` is your handle to everything: commands, UI, IPC, KV,
events. Full API: [`shell/docs/plugin-api.md`](../../shell/docs/plugin-api.md).

## 4. Build

```bash
pnpm install
pnpm build           # produces dist/index.js
```

## 5. Install into a forge

```bash
nexus plugin install ./
```

You'll see the manifest, the requested capabilities, and an approval
prompt. Approve it.

## 6. Run it

Open the desktop shell against the same forge:

```bash
nexus desktop --forge-path ~/scratch
```

Open the command palette (`Ctrl+Shift+P`), type "Hello", pick
**Hello: Say Hi**. You'll be prompted for a name; the toast follows.

## 7. Iterate

Edit `src/index.ts`, then:

```bash
pnpm build
nexus plugin install ./   # reinstalls + reloads in-place
```

For tighter feedback, `pnpm build --watch` keeps `dist/index.js`
fresh; `nexus plugin install ./ --watch` reloads the plugin on every
rebuild. No restart of Nexus required.

## Where to go next

- **You want to do something more interesting than a notification.**
  Read [Plugins / overview](plugins/overview.md) for the menu of
  capabilities, then [IPC](plugins/ipc.md) for how to call into the
  rest of Nexus.
- **You want to contribute UI.** Read
  [UI / views and slots](ui/views-and-slots.md).
- **You want to extend the editor.** Read
  [Editor / overview](editor/overview.md).
- **You want a more rigorous treatment of every step above.** Read
  [`../shell/writing-a-plugin.md`](../shell/writing-a-plugin.md)
  — the in-depth shell-plugin reference (manifest fields, activation
  events, sandbox model, capability declarations, slot system,
  worked word-count example).
