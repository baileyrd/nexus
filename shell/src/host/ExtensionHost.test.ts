// shell/src/host/ExtensionHost.test.ts
//
// WI-19 unit tests for ExtensionHost activation events.
//
// Sibling-of-implementation; surfaced to the default `pnpm test` glob
// via `tests/extension-host.test.ts` (mirrors the UriHandlerRegistry
// shim pattern).
//
// Coverage:
//   - Plugin with `['onStartup']` activates eagerly (preserved behaviour).
//   - Plugin with empty `activationEvents` activates eagerly (default).
//   - Plugin with `['onView:foo']` does NOT activate during loadAll.
//   - Firing `onView:foo` (via activationTriggers.fire) activates the
//     deferred plugin and removes it from the trigger map.
//   - Firing the same trigger twice only activates once.
//   - A plugin whose activate() throws is evicted from triggers and
//     subsequent fires are no-ops (no infinite retry).
//   - dependsOn on a lazy plugin promotes it eager (dep-graph wins).

import { test, beforeEach } from 'node:test'
import assert from 'node:assert/strict'
import { ExtensionHost } from './ExtensionHost.ts'
import { PluginRegistry } from './PluginRegistry.ts'
import { activationTriggers } from './ActivationTriggers.ts'
import { eventBus } from './EventBus.ts'
import type { Plugin, PluginAPI } from '../types/plugin.ts'

// ── helpers ──────────────────────────────────────────────────────────────────

function makePlugin(opts: {
  id: string
  activationEvents?: string[]
  dependsOn?: string[]
  onActivate?: (api: PluginAPI) => void | Promise<void>
}): Plugin {
  return {
    manifest: {
      id: opts.id,
      name: opts.id,
      version: '0.0.0',
      core: false,
      activationEvents: opts.activationEvents ?? ['onStartup'],
      dependsOn: opts.dependsOn,
    },
    activate: opts.onActivate ?? (() => {}),
  }
}

// Each test starts with a fresh host + registry + clean trigger maps.
// The trigger singleton is process-wide so we must reset between tests
// to avoid bleed-through.
function freshHost(): { host: ExtensionHost; registry: PluginRegistry } {
  activationTriggers.reset()
  const registry = new PluginRegistry()
  const host = new ExtensionHost(registry)
  return { host, registry }
}

beforeEach(() => {
  activationTriggers.reset()
})

// ── tests ────────────────────────────────────────────────────────────────────

test('plugin with [onStartup] activates eagerly during loadAll', async () => {
  const { host } = freshHost()
  let activated = false
  const p = makePlugin({
    id: 'eager.startup',
    activationEvents: ['onStartup'],
    onActivate: () => { activated = true },
  })
  await host.loadAll([p])
  assert.equal(activated, true)
  assert.equal(host.getState('eager.startup'), 'active')
})

test('plugin with empty activationEvents activates eagerly (default fallback)', async () => {
  const { host } = freshHost()
  let activated = false
  const p = makePlugin({
    id: 'eager.empty',
    activationEvents: [],
    onActivate: () => { activated = true },
  })
  await host.loadAll([p])
  assert.equal(activated, true)
})

test('plugin with [onView:foo] does NOT activate during loadAll', async () => {
  const { host } = freshHost()
  let activated = false
  const p = makePlugin({
    id: 'lazy.view',
    activationEvents: ['onView:foo'],
    onActivate: () => { activated = true },
  })
  await host.loadAll([p])
  assert.equal(activated, false)
  assert.equal(host.getState('lazy.view'), 'registered')
  // Trigger map should still hold the entry until something fires.
  assert.equal(activationTriggers.hasPending('onView:foo'), true)
})

test('firing onView:foo activates the deferred plugin and clears the trigger', async () => {
  const { host } = freshHost()
  let activated = false
  const p = makePlugin({
    id: 'lazy.view',
    activationEvents: ['onView:foo'],
    onActivate: () => { activated = true },
  })
  await host.loadAll([p])
  assert.equal(activated, false)

  await activationTriggers.fire('onView:foo')

  assert.equal(activated, true)
  assert.equal(host.getState('lazy.view'), 'active')
  // Once activated, the trigger entry is dropped — subsequent fires
  // become no-ops rather than re-activating an already-active plugin.
  assert.equal(activationTriggers.hasPending('onView:foo'), false)
})

test('firing the same trigger twice only activates once', async () => {
  const { host } = freshHost()
  let activations = 0
  const p = makePlugin({
    id: 'lazy.view',
    activationEvents: ['onView:bar'],
    onActivate: () => { activations++ },
  })
  await host.loadAll([p])
  await activationTriggers.fire('onView:bar')
  await activationTriggers.fire('onView:bar')
  assert.equal(activations, 1)
})

test('plugin with multiple triggers — first fire wins, others are evicted', async () => {
  const { host } = freshHost()
  let activations = 0
  const p = makePlugin({
    id: 'lazy.multi',
    activationEvents: ['onView:multi', 'onCommand:multi.show'],
    onActivate: () => { activations++ },
  })
  await host.loadAll([p])
  assert.equal(activationTriggers.hasPending('onView:multi'), true)
  assert.equal(activationTriggers.hasPending('onCommand:multi.show'), true)

  await activationTriggers.fire('onView:multi')
  assert.equal(activations, 1)
  // Both keys are gone — the plugin is loaded and the second trigger has
  // nothing left to wake.
  assert.equal(activationTriggers.hasPending('onView:multi'), false)
  assert.equal(activationTriggers.hasPending('onCommand:multi.show'), false)

  await activationTriggers.fire('onCommand:multi.show')
  assert.equal(activations, 1, 'second fire must not re-activate')
})

test('failed activation evicts triggers — no infinite retry on subsequent fires', async () => {
  const { host } = freshHost()
  let attempts = 0
  const p = makePlugin({
    id: 'lazy.broken',
    activationEvents: ['onCommand:broken'],
    onActivate: () => {
      attempts++
      throw new Error('boom')
    },
  })
  await host.loadAll([p])
  await activationTriggers.fire('onCommand:broken')
  assert.equal(attempts, 1)
  assert.equal(host.getState('lazy.broken'), 'error')
  assert.equal(activationTriggers.hasPending('onCommand:broken'), false)

  // Second fire is a no-op — trigger evicted, plugin in error state.
  await activationTriggers.fire('onCommand:broken')
  assert.equal(attempts, 1, 'failed plugin must not be retried by trigger')
})

test('eager plugin depending on a lazy plugin promotes the dep to eager', async () => {
  const { host } = freshHost()
  let depActivated = false
  let consumerActivated = false
  const dep = makePlugin({
    id: 'lazy.dep',
    activationEvents: ['onView:never-fired'],
    onActivate: () => { depActivated = true },
  })
  const consumer = makePlugin({
    id: 'eager.consumer',
    activationEvents: ['onStartup'],
    dependsOn: ['lazy.dep'],
    onActivate: () => { consumerActivated = true },
  })
  await host.loadAll([dep, consumer])
  // dependsOn forces dep to load even though its declared trigger never fires.
  assert.equal(depActivated, true, 'dep promoted to eager via dependsOn')
  assert.equal(consumerActivated, true)
  assert.equal(host.getState('lazy.dep'), 'active')
  // The redundant trigger is cleared on activation.
  assert.equal(activationTriggers.hasPending('onView:never-fired'), false)
})

test('mixed-trigger plugin: onStartup wins, lazy keys still recorded but evicted on activation', async () => {
  const { host } = freshHost()
  let activations = 0
  // A plugin can declare both eager + lazy events. Eager wins (loaded
  // at boot); the lazy keys are recorded but cleaned up on activation
  // so subsequent fires don't try to re-load it.
  const p = makePlugin({
    id: 'mixed',
    activationEvents: ['onStartup', 'onCommand:mixed.show'],
    onActivate: () => { activations++ },
  })
  await host.loadAll([p])
  assert.equal(activations, 1)
  assert.equal(activationTriggers.hasPending('onCommand:mixed.show'), false)

  await activationTriggers.fire('onCommand:mixed.show')
  assert.equal(activations, 1)
})

test('two plugins gated on the same trigger both wake on a single fire', async () => {
  const { host } = freshHost()
  const seen: string[] = []
  const a = makePlugin({
    id: 'lazy.a',
    activationEvents: ['onView:shared'],
    onActivate: () => { seen.push('a') },
  })
  const b = makePlugin({
    id: 'lazy.b',
    activationEvents: ['onView:shared'],
    onActivate: () => { seen.push('b') },
  })
  await host.loadAll([a, b])
  assert.equal(seen.length, 0)
  await activationTriggers.fire('onView:shared')
  assert.deepEqual(seen.sort(), ['a', 'b'])
  assert.equal(activationTriggers.hasPending('onView:shared'), false)
})

// ── WI-35 — per-plugin crash quarantine ─────────────────────────────────────

test('WI-35 — a plugin whose activate() throws does not abort sibling eager loads', async () => {
  const { host } = freshHost()
  const order: string[] = []
  const bad = makePlugin({
    id: 'wi35.bad',
    activationEvents: ['onStartup'],
    onActivate: () => {
      order.push('bad')
      throw new Error('boom')
    },
  })
  const good = makePlugin({
    id: 'wi35.good',
    activationEvents: ['onStartup'],
    onActivate: () => { order.push('good') },
  })
  // `bad` first so a naïve propagating activate() would kill `good`.
  await host.loadAll([bad, good])
  assert.equal(host.getState('wi35.bad'), 'error')
  assert.equal(host.getState('wi35.good'), 'active')
  assert.deepEqual(order, ['bad', 'good'])
})

test('WI-35 — activate() failure cleans up contributions and fires plugin:error', async () => {
  const { host, registry } = freshHost()
  const errors: Array<{ pluginId: string; error: Error }> = []
  const unsub = eventBus.on<{ pluginId: string; error: Error }>(
    'plugin:error',
    (e) => { errors.push(e) },
  )
  try {
    const p: Plugin = {
      manifest: {
        id: 'wi35.contribs',
        name: 'wi35.contribs',
        version: '0.0.0',
        core: false,
        activationEvents: ['onStartup'],
        contributes: {
          commands: [{ id: 'wi35.contribs.hello', title: 'Hello' }],
        },
      },
      activate: () => { throw new Error('halfway') },
    }
    await host.loadAll([p])
    assert.equal(host.getState('wi35.contribs'), 'error')
    // Contributions swept — the manifest-registered command is gone,
    // so a later retry (e.g. user reload) can re-register cleanly.
    assert.equal(registry.commands.has('wi35.contribs.hello'), false)
    assert.equal(errors.length, 1)
    assert.equal(errors[0].pluginId, 'wi35.contribs')
    assert.match(errors[0].error.message, /halfway/)
  } finally {
    unsub()
  }
})

test('WI-35 — EventBus.emit: a throwing listener does not stop sibling listeners', () => {
  const seen: string[] = []
  const unsubA = eventBus.on('wi35.topic', () => {
    seen.push('a')
    throw new Error('listener-a-boom')
  })
  const unsubB = eventBus.on('wi35.topic', () => { seen.push('b') })
  try {
    // Must not throw — EventBus swallows per-listener failures.
    eventBus.emit('wi35.topic', {})
    // Order is insertion order; both fired even though 'a' panicked.
    assert.deepEqual(seen, ['a', 'b'])
  } finally {
    unsubA()
    unsubB()
  }
})
