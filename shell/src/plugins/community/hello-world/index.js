// shell/src/plugins/community/hello-world/index.js
//
// F-8.1.1-fo1 — migrated to consume the bundled sandbox runtime.
//
// This bundle is loaded by `SandboxOrchestrator.buildSandboxSrcDoc`'s
// inline boot script, which dynamic-imports the runtime first
// (`runtime.ts` from `@nexus/extension-api/sandbox/runtime`, bundled
// by `shell/vite.sandbox-runtime-plugin.ts` and shipped to the iframe
// via the `runtimeUrl` parameter) and then dynamic-imports this
// bundle. The boot script reads `bundle.default`, hands it to
// `runtime.bootstrapSandboxedPlugin(plugin)`, and the runtime takes
// over: posting the handshake hello, marshalling RPC, building the
// `SandboxedPluginContext` proxy, and dispatching events.
//
// Authoring shape: this file is a hand-written equivalent of the
// bundler output of `index.ts`. Once a per-plugin bundler lands, the
// shape stays the same — only the source of the file changes (TS →
// generated JS). `index.ts` is the spec for that future cutover.
//
// What changed vs. the pre-F-8.1.1-fo1 stepping stone:
//   - Dropped the hand-rolled handshake / postMessage / dispatch loop
//     (~270 lines, see git history). The runtime now owns all of it.
//   - Plugin author code is just `activate(ctx)` against the typed
//     `SandboxedPluginContext` from `@nexus/extension-api/sandbox`.
//
// Manifest invariants (unchanged):
//   - `sandboxed: true`     — routes through SandboxOrchestrator.
//   - `apiVersion: 1`       — matches PLUGIN_API_VERSION in the shell.
//   - `capabilities:
//       ["UiNotify"]`       — guards `notifications.show`, the only
//                             capability-gated host surface used here.

const plugin = {
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
