// shell/src/stores/pluginsStatusStore.test.ts
//
// OI-09 — pluginsStatusStore aggregates `plugin:activated` /
// `plugin:deactivated` / `plugin:error` events from the EventBus so
// observability surfaces (Extensions Settings tab, future status bar
// widget) can render plugin lifecycle without poking ExtensionHost
// directly.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { eventBus } from '../host/EventBus'
import {
  usePluginsStatusStore,
  getPluginStatus,
} from './pluginsStatusStore'

function reset() {
  usePluginsStatusStore.getState()._reset()
}

test('plugin:activated transitions a plugin to active', () => {
  reset()
  eventBus.emit('plugin:activated', { pluginId: 'nexus.foo' })
  assert.deepEqual(getPluginStatus('nexus.foo'), { state: 'active' })
})

test('plugin:deactivated transitions a plugin to inactive', () => {
  reset()
  eventBus.emit('plugin:activated', { pluginId: 'nexus.foo' })
  eventBus.emit('plugin:deactivated', { pluginId: 'nexus.foo' })
  assert.deepEqual(getPluginStatus('nexus.foo'), { state: 'inactive' })
})

test('plugin:error captures the message and stack', () => {
  reset()
  const err = new Error('boom')
  eventBus.emit('plugin:error', { pluginId: 'nexus.bad', error: err })
  const status = getPluginStatus('nexus.bad')
  assert.equal(status?.state, 'error')
  assert.equal(status?.lastError?.message, 'boom')
  assert.equal(typeof status?.lastError?.stack, 'string')
})

test('a recovered plugin clears its lastError on next activation', () => {
  reset()
  const err = new Error('first try failed')
  eventBus.emit('plugin:error', { pluginId: 'nexus.flaky', error: err })
  assert.equal(getPluginStatus('nexus.flaky')?.state, 'error')
  assert.equal(getPluginStatus('nexus.flaky')?.lastError?.message, 'first try failed')

  // Successful re-activation (e.g. after hot-reload).
  eventBus.emit('plugin:activated', { pluginId: 'nexus.flaky' })
  const status = getPluginStatus('nexus.flaky')
  assert.equal(status?.state, 'active')
  assert.equal(status?.lastError, undefined)
})

test('multiple plugins coexist independently in the store', () => {
  reset()
  eventBus.emit('plugin:activated', { pluginId: 'nexus.a' })
  eventBus.emit('plugin:error', { pluginId: 'nexus.b', error: new Error('nope') })
  eventBus.emit('plugin:activated', { pluginId: 'nexus.c' })

  const byId = usePluginsStatusStore.getState().byId
  assert.deepEqual(Object.keys(byId).sort(), ['nexus.a', 'nexus.b', 'nexus.c'])
  assert.equal(byId['nexus.a'].state, 'active')
  assert.equal(byId['nexus.b'].state, 'error')
  assert.equal(byId['nexus.b'].lastError?.message, 'nope')
  assert.equal(byId['nexus.c'].state, 'active')
})

test('store updates immutably — old reference does not mutate', () => {
  reset()
  eventBus.emit('plugin:activated', { pluginId: 'nexus.x' })
  const before = usePluginsStatusStore.getState().byId
  eventBus.emit('plugin:activated', { pluginId: 'nexus.y' })
  const after = usePluginsStatusStore.getState().byId

  // The slice we captured before the second event must NOT have grown
  // (zustand returned a fresh object on the second update).
  assert.notEqual(before, after)
  assert.equal(Object.keys(before).length, 1)
  assert.equal(Object.keys(after).length, 2)
})
