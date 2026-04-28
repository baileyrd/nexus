// shell/src/plugins/community/hello-world/index.ts
//
// F-8.1.1-fo1 — idiomatic SandboxedPlugin source.
//
// This file is the *target* author shape for the plugin once a
// per-plugin bundler lands. It is not loaded at runtime; the
// SandboxOrchestrator dynamic-imports `index.js` (the hand-written
// equivalent of the bundler output of this file) inside the iframe.
//
// The runtime contract:
//   - The host loads the precompiled `bootstrapSandboxedPlugin` runtime
//     separately via the `runtimeUrl` channel (see
//     `shell/vite.sandbox-runtime-plugin.ts` + `getRuntimeUrl` in
//     `shell/src/main.tsx`).
//   - The plugin bundle exports a {@link SandboxedPlugin} as `default`
//     and does NOT call `bootstrapSandboxedPlugin` itself — the
//     orchestrator's srcdoc reads `bundle.default` and passes it to
//     `runtime.bootstrapSandboxedPlugin(plugin)`.
//
// Migration path (now narrower than under F-8.1.1):
//   1. Add a per-plugin build step (esbuild / vite-plugin) that emits
//      this file as ESM, with type-only imports stripped. The runtime
//      stays out-of-bundle — no need to inline `@nexus/extension-api`.
//   2. Point the manifest's `main` at the bundler output.
//   3. Delete `index.js` (this file becomes the sole source).

import type { SandboxedPlugin } from '@nexus/extension-api'

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

export default plugin
