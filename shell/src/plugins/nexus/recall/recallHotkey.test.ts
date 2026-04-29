// shell/src/plugins/nexus/recall/recallHotkey.test.ts
//
// FU-9 — recall hotkey live-rebind. Boots the plugin against a
// stubbed PluginAPI and verifies that:
//
//   1. a persisted `recall.hotkey` value at activate time is applied
//      to `api.keybindings.setOverride`
//   2. a non-empty config change pushes a new override
//   3. a blank/empty change clears the override
//
// We don't exercise `setRecallApi` or the overlay slot here — both
// are covered by `recallStore.test.ts` / `recallRuntime.test.ts`.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { recallPlugin } from './index.ts'

interface SetCall { commandId: string; chord: string }

function stubApi(opts: { persisted?: string | null } = {}) {
  const setCalls: SetCall[] = []
  const clearCalls: string[] = []
  const configHandlers = new Map<string, (v: unknown) => void>()
  const recallStoreSubs: Array<() => void> = []
  const visibleByKey = new Map<string, unknown>()
  const noop = () => undefined

  const api = {
    commands: { register: noop, execute: async () => undefined, all: () => [] },
    views: { register: noop },
    workspace: {} as unknown,
    viewRegistry: {} as unknown,
    context: {
      set: (k: string, v: unknown) => { visibleByKey.set(k, v) },
      get: (k: string) => visibleByKey.get(k),
      evaluate: () => false,
    },
    events: { on: () => () => undefined, emit: noop },
    storage: { get: () => null, set: noop, delete: noop, clear: noop },
    statusBar: { createItem: () => ({ dispose: noop } as never) },
    configuration: {
      register: noop,
      getValue: <T,>(key: string, def: T): T => {
        if (key === 'recall.hotkey' && opts.persisted !== undefined) {
          return (opts.persisted as unknown as T) ?? def
        }
        return def
      },
      setValue: noop,
      onChange: (key: string, handler: (v: unknown) => void) => {
        configHandlers.set(key, handler)
        return () => configHandlers.delete(key)
      },
    },
    keybindings: {
      setOverride: async (commandId: string, chord: string) => {
        setCalls.push({ commandId, chord })
      },
      clearOverride: async (commandId: string) => {
        clearCalls.push(commandId)
      },
    },
    notifications: { show: noop },
    fs: {} as never,
    kernel: {
      invoke: async () => ({} as never),
      on: async () => () => undefined,
      available: async () => false,
    },
    platform: {} as never,
    activityBar: { addItem: noop, removeItem: noop },
    input: { prompt: async () => null, confirm: async () => false },
    settings: { registerTab: noop },
    uri: { register: () => () => undefined },
    editor: {
      active: () => null,
      onChange: () => () => undefined,
      registerFencedCodeRenderer: () => () => undefined,
    },
  }

  return {
    api: api as unknown as Parameters<typeof recallPlugin.activate>[0],
    setCalls,
    clearCalls,
    fireConfigChange: (val: unknown) =>
      configHandlers.get('recall.hotkey')?.(val),
    recallStoreSubs,
  }
}

test('recall plugin applies persisted recall.hotkey at activate', async () => {
  const ctx = stubApi({ persisted: 'mod-shift-q' })
  await recallPlugin.activate(ctx.api)
  const initial = ctx.setCalls.find(c => c.commandId === 'nexus.recall.open')
  assert.ok(initial, 'setOverride must be called for the persisted value')
  assert.equal(initial!.chord, 'mod-shift-q')
})

test('recall plugin pushes setOverride on a non-empty config change', async () => {
  const ctx = stubApi({ persisted: null })
  await recallPlugin.activate(ctx.api)
  // Persisted was null → no initial setOverride.
  assert.equal(
    ctx.setCalls.filter(c => c.commandId === 'nexus.recall.open').length,
    0,
  )
  ctx.fireConfigChange('alt-shift-r')
  assert.deepEqual(
    ctx.setCalls.find(c => c.commandId === 'nexus.recall.open'),
    { commandId: 'nexus.recall.open', chord: 'alt-shift-r' },
  )
})

test('recall plugin clears the override when the config value is blank', async () => {
  const ctx = stubApi({ persisted: 'mod-shift-q' })
  await recallPlugin.activate(ctx.api)
  ctx.fireConfigChange('')
  assert.ok(
    ctx.clearCalls.includes('nexus.recall.open'),
    'clearOverride must run for an empty hotkey',
  )
  ctx.fireConfigChange('   ')
  assert.equal(
    ctx.clearCalls.filter(c => c === 'nexus.recall.open').length,
    2,
    'whitespace-only also clears',
  )
})
