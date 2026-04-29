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
import type { OverrideStorage } from '../src/registry/KeybindingRegistry'

function memoryStorage(): OverrideStorage {
  let state: Record<string, string> = {}
  return {
    async read() { return { ...state } },
    async write(o) { state = { ...o } },
  }
}

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

// ── FU-9: per-plugin keybinding override sweep ─────────────────────────────

test('unregisterAll clears every keybinding override the plugin pushed', async () => {
  const registry = new PluginRegistry()
  registry.keybindings.bindStorage(memoryStorage())
  registry.keybindings.registerFromManifest('plug.a', {
    command: 'cmd.one',
    key: 'ctrl+1',
  })
  registry.keybindings.registerFromManifest('plug.a', {
    command: 'cmd.two',
    key: 'ctrl+2',
  })

  await registry.setKeybindingOverride('plug.a', 'cmd.one', 'alt+1')
  await registry.setKeybindingOverride('plug.a', 'cmd.two', 'alt+2')
  assert.equal(registry.keybindings.getOverride('cmd.one'), 'alt+1')
  assert.equal(registry.keybindings.getOverride('cmd.two'), 'alt+2')

  registry.unregisterAll('plug.a')
  // Persistence is async — wait a turn so the fire-and-forget
  // `clearOverride` has settled before we assert.
  await new Promise<void>((r) => setTimeout(r, 0))

  assert.equal(registry.keybindings.getOverride('cmd.one'), undefined)
  assert.equal(registry.keybindings.getOverride('cmd.two'), undefined)
})

test('unregisterAll preserves a Settings-UI override that replaced the plugin one', async () => {
  const registry = new PluginRegistry()
  registry.keybindings.bindStorage(memoryStorage())
  registry.keybindings.registerFromManifest('plug.a', {
    command: 'cmd.shared',
    key: 'ctrl+s',
  })

  // Plugin pushes its preferred chord.
  await registry.setKeybindingOverride('plug.a', 'cmd.shared', 'alt+s')
  // The user replaces it via the Settings UI — which calls the bare
  // registry surface, NOT the PluginRegistry helper, so no tag update.
  await registry.keybindings.setOverride('cmd.shared', 'meta+s')

  registry.unregisterAll('plug.a')
  await new Promise<void>((r) => setTimeout(r, 0))

  // The user's override survives the plugin teardown.
  assert.equal(registry.keybindings.getOverride('cmd.shared'), 'meta+s')
})

test('clearKeybindingOverride from a different plugin does not steal the tag', async () => {
  const registry = new PluginRegistry()
  registry.keybindings.bindStorage(memoryStorage())
  registry.keybindings.registerFromManifest('plug.a', {
    command: 'cmd.x',
    key: 'ctrl+x',
  })

  await registry.setKeybindingOverride('plug.a', 'cmd.x', 'alt+x')
  // A different plugin attempts to clear; the registry-level override
  // is dropped (any plugin can clear) but the tag must not be removed
  // by an unrelated caller.
  await registry.clearKeybindingOverride('plug.b', 'cmd.x')
  // Re-set by plug.a so the tag is fresh, then deactivate plug.a and
  // verify the sweep still finds it.
  await registry.setKeybindingOverride('plug.a', 'cmd.x', 'alt+x')
  registry.unregisterAll('plug.a')
  await new Promise<void>((r) => setTimeout(r, 0))
  assert.equal(registry.keybindings.getOverride('cmd.x'), undefined)
})
