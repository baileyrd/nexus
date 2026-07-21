// {{plugin-name}} — smoke test.
//
// Exercises `activate(ctx)` against a minimal fake `SandboxedPluginContext`
// so `pnpm test` catches an obviously broken command registration or panel
// render before the plugin ever reaches a real host.
//
// This imports the *bundled* `.test-bundle.mjs` (built by the `pretest`
// script), not `./index.ts` directly: `index.ts` calls
// `bootstrapSandboxedPlugin(plugin)` at module scope, which pulls in
// `@nexus/extension-api`'s sandbox runtime and expects real `window`/
// `postMessage` globals — `./test-setup.ts` (registered via `--import`)
// supplies those via happy-dom. Bundling first also sidesteps
// `@nexus/extension-api`'s bundler-oriented module layout, which isn't
// resolvable by Node's own ESM loader without a bundler in the loop.
// There are no static types for the bundle output, hence the
// `@ts-expect-error` below — extend this file with assertions specific
// to your plugin's behavior.

import assert from 'node:assert/strict'
import { test } from 'node:test'
// @ts-expect-error - built by `pretest`; no static .d.ts to check against.
import plugin from './.test-bundle.mjs'

type CommandHandler = (...args: unknown[]) => unknown

function fakeContext() {
  const commands = new Map<string, CommandHandler>()
  let lastPanelId: string | undefined
  let lastPanelRender: (() => unknown) | undefined
  return {
    ctx: {
      pluginId: '{{plugin-id}}',
      commands: {
        register(id: string, handler: CommandHandler) {
          commands.set(id, handler)
          return { dispose() {} }
        },
        async execute(id: string, ...args: unknown[]) {
          const handler = commands.get(id)
          if (!handler) throw new Error(`no handler registered for '${id}'`)
          return handler(...args)
        },
      },
      notifications: {
        async show() {},
      },
      views: {
        registerPanel(viewId: string, render: () => unknown) {
          lastPanelId = viewId
          lastPanelRender = render
          return { dispose() {} }
        },
      },
    },
    panel: () => ({ id: lastPanelId, render: lastPanelRender }),
  }
}

test('activate registers the hello command', async () => {
  const { ctx } = fakeContext()
  await plugin.activate(ctx)
  await ctx.commands.execute('{{plugin-id}}.hello')
})

test('activate registers a panel that renders without throwing', async () => {
  const { ctx, panel } = fakeContext()
  await plugin.activate(ctx)
  const { id, render } = panel()
  assert.equal(id, '{{plugin-id}}.panel')
  assert.ok(render, 'expected registerPanel to be called')
  const tree = render!() as { type: string }
  assert.equal(tree.type, 'vstack')
})
