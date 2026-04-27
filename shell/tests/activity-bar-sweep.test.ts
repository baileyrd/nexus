/**
 * Regression: disabling a plugin via Settings → Plugins must remove the
 * plugin's rail icons. Activity-bar items are tracked by `pluginId` in
 * `PluginRegistry` (via `registry.track(pluginId, 'activityBar:<id>')`
 * inside `api.activityBar.addItem`), and `unregisterAll` emits
 * `activityBar:itemRemoved` for each so the ActivityBar Zustand store
 * drops the matching row.
 */

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { PluginRegistry } from '../src/host/PluginRegistry'
import { eventBus } from '../src/host/EventBus'

test('unregisterAll emits activityBar:itemRemoved for each tracked item', () => {
  const registry = new PluginRegistry()
  const removed: string[] = []
  const off = eventBus.on('activityBar:itemRemoved', (p: { id: string }) => {
    removed.push(p.id)
  })

  registry.track('plugin.a', 'activityBar:rail.a-one')
  registry.track('plugin.a', 'activityBar:rail.a-two')
  registry.track('plugin.b', 'activityBar:rail.b-only')

  registry.unregisterAll('plugin.a')
  off()

  assert.deepEqual(
    [...removed].sort(),
    ['rail.a-one', 'rail.a-two'],
    'both plugin.a items must be swept; plugin.b must be untouched',
  )
})

test('unregisterAll on a plugin with no activity-bar items is a no-op', () => {
  const registry = new PluginRegistry()
  let fired = 0
  const off = eventBus.on('activityBar:itemRemoved', () => { fired++ })

  registry.track('plugin.x', 'command:plugin.x.doThing')
  registry.unregisterAll('plugin.x')
  off()

  assert.equal(fired, 0)
})
