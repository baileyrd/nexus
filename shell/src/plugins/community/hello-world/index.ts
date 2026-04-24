// shell/src/plugins/community/hello-world/index.ts
//
// WI-30e — hello-world, idiomatic SandboxedPlugin source.
//
// This file is the *target* shape for the plugin once a sandbox
// bundler lands. It is NOT loaded at runtime today — the
// SandboxOrchestrator dynamic-imports `index.js` inside the iframe,
// and bare-specifier imports like `@nexus/extension-api` cannot
// resolve in a null-origin iframe without either an import map or a
// pre-bundled output. Until that bundler exists, `index.js` hand-rolls
// the protocol; this file documents what the generated output should
// be equivalent to.
//
// Migration path (tracked in docs/wi30-sandbox-design.md §7):
//   1. Introduce a plugin-build step (esbuild or vite-plugin) that
//      bundles this file together with the `bootstrapSandboxedPlugin`
//      runtime from `@nexus/extension-api/sandbox/runtime` into a
//      single `index.bundle.js`.
//   2. Point the manifest's `main` at that bundle.
//   3. Delete `index.js` (this file becomes the sole source).

import {
  bootstrapSandboxedPlugin,
  type SandboxedPlugin,
} from '@nexus/extension-api'

const plugin: SandboxedPlugin = {
  async activate(ctx) {
    await ctx.notifications.show({
      message: 'Hello from the sandbox!',
      type: 'info',
      duration: 3000,
    })

    ctx.commands.register('hello.greet', async () => {
      const name = await ctx.input.prompt('Your name?', 'e.g. Ada')
      if (name) {
        await ctx.notifications.show({
          message: `Hi ${name}!`,
          type: 'info',
          duration: 3000,
        })
      }
    })

    ctx.views.registerPanel('hello.panel', () => ({
      type: 'vstack',
      gap: 8,
      children: [
        { type: 'heading', value: 'Hello', level: 2 },
        { type: 'text', value: 'Click the button to greet someone.' },
        { type: 'button', label: 'Greet', commandId: 'hello.greet' },
      ],
    }))
  },
}

bootstrapSandboxedPlugin(plugin)

export default plugin
