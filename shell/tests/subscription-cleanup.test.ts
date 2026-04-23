/**
 * Phase 1 acceptance gap #4 — subscription cleanup.
 *
 * `PluginRegistry.unregisterAll(pluginId)` is called by the ExtensionHost on
 * plugin unload. It must drain every kernel-bus subscription tracked under
 * `pluginId` so the underlying Rust forwarder tasks get torn down and dead
 * listeners stop receiving events.
 *
 * Wiring landed in commit c4d31d3 (PluginRegistry.trackSubscription +
 * unregisterAll sweep + idempotent disposer in api.kernel.on). These tests
 * prove the contract holds:
 *
 *   1. drains all disposers tracked under the target pluginId
 *   2. leaves disposers tracked under OTHER pluginIds untouched
 *   3. is safe when no subscriptions are tracked (empty / unknown plugin)
 *   4. wraps each disposer in try/catch so one bad disposer doesn't strand
 *      its siblings (PluginRegistry.ts §unregisterAll requires this)
 *   5. is idempotent — a second unregisterAll on the same plugin is a no-op
 *
 * These are unit-level checks against PluginRegistry — no Tauri runtime, no
 * kernel boot. Any regression in the cleanup path will fail here in O(ms).
 */

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { PluginRegistry } from '../src/host/PluginRegistry'

test('PluginRegistry.unregisterAll drains tracked subscriptions', () => {
  const registry = new PluginRegistry()
  let aFired = 0
  let bFired = 0
  const otherFired = { count: 0 }

  registry.trackSubscription('plugin.a', () => { aFired++ })
  registry.trackSubscription('plugin.a', () => { aFired++ })
  registry.trackSubscription('plugin.b', () => { bFired++ })
  registry.trackSubscription('plugin.other', () => { otherFired.count++ })

  registry.unregisterAll('plugin.a')

  assert.equal(aFired, 2, 'both plugin.a disposers must fire')
  assert.equal(bFired, 0, 'plugin.b unrelated — must NOT fire')
  assert.equal(otherFired.count, 0, 'plugin.other unrelated — must NOT fire')
})

test('unregisterAll is safe when no subscriptions tracked', () => {
  const registry = new PluginRegistry()
  // No throw; idempotent on empty.
  registry.unregisterAll('nonexistent')
})

test('a disposer that throws does not strand its siblings', () => {
  const registry = new PluginRegistry()
  let secondFired = false
  registry.trackSubscription('plugin.x', () => { throw new Error('boom') })
  registry.trackSubscription('plugin.x', () => { secondFired = true })

  // Must not throw out of unregisterAll — PluginRegistry wraps each disposer
  // in try/catch so one failure can't abort the plugin-unload path.
  registry.unregisterAll('plugin.x')

  assert.equal(secondFired, true, 'sibling disposer must still fire after one throws')
})

test('a second unregisterAll on the same plugin is a no-op (idempotent)', () => {
  const registry = new PluginRegistry()
  let count = 0
  registry.trackSubscription('plugin.y', () => { count++ })
  registry.unregisterAll('plugin.y')
  registry.unregisterAll('plugin.y')
  assert.equal(count, 1, 'disposer must fire exactly once')
})
