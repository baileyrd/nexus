# Writing your first Nexus plugin

Sandboxed JS/TS community plugins are the modern path for extending the
Nexus shell. This tutorial is the quickstart: scaffold, build, install,
run. For the in-depth reference (activation events, capabilities, slot
system, sandbox model) see
[`../../shell/docs/writing-a-plugin.md`](../../shell/docs/writing-a-plugin.md)
once you have a scaffold in hand.

> **Audience.** You know a little TypeScript and want a command + a panel
> view in the shell. For a pure-Rust WASM plugin (maximum-trust or
> capability-gated), see the `core` / `community` templates instead —
> covered at the end of this doc. For deeper API coverage see the
> [reference tutorial](../../shell/docs/writing-a-plugin.md).

## Prerequisites

- Node 18+ with pnpm (`npm i -g pnpm` if you don't have it).
- A built `nexus` binary on your `PATH` (Phase 4 WI-38 unified binary).
- The Nexus shell installed somewhere runnable — the shell's plugin
  directory lives at `~/.nexus-shell/plugins/` on Linux / macOS and
  `%USERPROFILE%/.nexus-shell/plugins/` on Windows.

## 1. Scaffold

```sh
nexus plugin scaffold \
  --template script \
  --id com.example.hello \
  --name "Hello"
```

`--template script` is the default as of WI-39; pass it explicitly in
scripts and CI for clarity. The scaffold emits five files under
`./com.example.hello/`:

```
com.example.hello/
├── plugin.json      # sandboxed-plugin manifest (apiVersion 1, sandboxed: true)
├── index.ts         # your source — activate(ctx) goes here
├── package.json     # pnpm scripts + pinned @nexus/extension-api
├── tsconfig.json    # ES2020 / strict
└── README.md        # authoring + install guide
```

Other flags:

| Flag           | Purpose                                              | Default                  |
|----------------|------------------------------------------------------|--------------------------|
| `--template`   | `script` / `core` / `community` (alias: `--type`)    | `script`                 |
| `--id`         | Reverse-DNS plugin id (`^[a-z0-9]+(…)\.[a-z0-9]+…$`) | *(required)*             |
| `--name`       | Human-readable display name                          | *(required)*             |
| `--author`     | Author name / e-mail                                 | `Unknown`                |
| `--output` `-o`| Output directory                                     | `./<id>/`                |

## 2. Edit `index.ts`

The scaffolded `index.ts` registers one command and one panel view. The
shape is always the same:

```ts
import {
  bootstrapSandboxedPlugin,
  type SandboxedPlugin,
} from '@nexus/extension-api'

const plugin: SandboxedPlugin = {
  async activate(ctx) {
    ctx.commands.register('com.example.hello.greet', async () => {
      await ctx.notifications.show({
        message: 'Hello from sandbox!',
        type: 'info',
        duration: 3000,
      })
    })

    ctx.views.registerPanel('com.example.hello.panel', () => ({
      type: 'vstack',
      gap: 8,
      children: [
        { type: 'heading', value: 'Hello', level: 2 },
        { type: 'button', label: 'Greet', commandId: 'com.example.hello.greet' },
      ],
    }))
  },
}

bootstrapSandboxedPlugin(plugin)
export default plugin
```

`SandboxedPluginContext` (the type of `ctx`) exposes `commands`,
`notifications`, `views`, `storage`, `statusBar`, `kernel.invoke`, and
friends. See `packages/nexus-extension-api/src/sandbox/context.ts` for the
full surface.

Panels render via the declarative `PanelNode` tree — no React crosses the
sandbox boundary.

## 3. Install dependencies and build

```sh
cd com.example.hello
pnpm install
pnpm build
```

`pnpm build` runs esbuild to produce `index.js` — a single self-contained
ESM bundle that the shell loads inside a null-origin iframe.

## 4. Install into the shell

Drop the built artifacts into the shell's plugin directory:

```sh
mkdir -p ~/.nexus-shell/plugins/com.example.hello
cp index.js plugin.json ~/.nexus-shell/plugins/com.example.hello/
```

Confirm the install:

```sh
nexus plugin list --shell
# ID                           Name         Version
# com.example.hello            Hello        0.1.0
```

Launch the shell (`nexus desktop`). Your command shows up in the command
palette; your panel shows up where the shell mounts plugin-registered
panels.

## Declaring capabilities

The `plugin.json` scaffold ships with `"capabilities": []`. The host
denies any gated API call your plugin hasn't declared — for example
reading the kernel bridge requires `Kernel`, hitting the platform
adapter requires `Platform`, etc. Add the capabilities you actually use:

```json
{
  "capabilities": ["UiNotify", "Storage"]
}
```

The active capability taxonomy lives in `crates/nexus-plugin-api` — see
`Capability` in the generated `ts-rs` bindings.

## Uninstall

```sh
nexus plugin remove com.example.hello
```

## Other templates

The `script` template is the recommended path for most community plugins.
Two legacy templates are still supported for pure-Rust authors:

- `nexus plugin scaffold --template core ...` — maximum-trust WASM plugin.
- `nexus plugin scaffold --template community ...` — capability-gated WASM
  plugin.

Both emit `Cargo.toml` + `manifest.toml` + `src/lib.rs` and target the
kernel-side WASM loader rather than the shell's sandboxed iframe runtime.
Use them when you need `wasmtime` fuel semantics or direct access to
kernel host functions that aren't exposed across the `postMessage`
boundary.

## Next steps

Once the hello scaffold runs, head to
[`../../shell/docs/writing-a-plugin.md`](../../shell/docs/writing-a-plugin.md)
for the reference on activation events, capability declarations, slot
surfaces, and the sandbox contract.
