// shell/src/registry/KeybindingRegistry.test.ts
//
// WI-04 unit tests for the override layer. Verifies that:
//   - setOverride is reflected by getAllBindings + getOverride
//   - clearOverride reverts the active chord to the manifest default
//   - the persistence round-trip (setOverride → fresh registry →
//     loadOverrides) restores the override
//   - normalizeChord canonicalises modifier aliases (cmd → meta, etc.)
//
// The test runner is `node --import tsx --test`.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  KeybindingRegistry,
  normalizeChord,
  formatChord,
  type OverrideStorage,
} from './KeybindingRegistry.ts'

// Minimal in-memory storage adapter — mirrors the contract the
// settings plugin's localStorage-backed adapter implements at runtime.
function memoryStorage(initial: Record<string, string> = {}): OverrideStorage & {
  state: Record<string, string>
} {
  const state = { ...initial }
  return {
    state,
    async read() {
      return { ...state }
    },
    async write(overrides) {
      // Replace, don't merge — matches the settings adapter, which
      // writes the whole map every time.
      for (const k of Object.keys(state)) delete state[k]
      Object.assign(state, overrides)
    },
  }
}

function freshRegistry(): KeybindingRegistry {
  const reg = new KeybindingRegistry()
  reg.registerFromManifest('plug.a', { command: 'cmd.alpha', key: 'ctrl+a' })
  reg.registerFromManifest('plug.b', { command: 'cmd.bravo', key: 'ctrl+shift+b' })
  return reg
}

function freshRegistryWithStorage(
  storage: ReturnType<typeof memoryStorage>,
): KeybindingRegistry {
  const reg = freshRegistry()
  reg.bindStorage(storage)
  return reg
}

test('normalizeChord folds modifier aliases and orders modifiers', () => {
  // Canonical modifier order: ctrl, shift, alt, meta, then key.
  assert.equal(normalizeChord('Cmd+Shift+K'), 'shift+meta+k')
  assert.equal(normalizeChord('SHIFT+CMD+K'), 'shift+meta+k')
  assert.equal(normalizeChord('Ctrl+K'), 'ctrl+k')
  assert.equal(normalizeChord('alt+ctrl+x'), 'ctrl+alt+x')
})

test('formatChord: Title-Cases each part', () => {
  assert.equal(formatChord('shift+meta+k'), 'Shift+Meta+K')
  assert.equal(formatChord('ctrl+/'), 'Ctrl+/')
  assert.equal(formatChord(''), '')
})

test('setOverride is reflected by getAllBindings + getOverride', async () => {
  const storage = memoryStorage()
  const reg = freshRegistryWithStorage(storage)

  await reg.setOverride('cmd.alpha', 'ctrl+shift+a')

  assert.equal(reg.getOverride('cmd.alpha'), 'ctrl+shift+a')

  const rows = reg.getAllBindings()
  const alpha = rows.find(r => r.commandId === 'cmd.alpha')
  assert.ok(alpha)
  assert.equal(alpha!.current, 'ctrl+shift+a')
  assert.equal(alpha!.default, 'ctrl+a')
  assert.equal(alpha!.overridden, true)

  const bravo = rows.find(r => r.commandId === 'cmd.bravo')
  assert.ok(bravo)
  assert.equal(bravo!.overridden, false)
  assert.equal(bravo!.current, bravo!.default)
})

test('clearOverride reverts to the manifest default', async () => {
  const storage = memoryStorage()
  const reg = freshRegistryWithStorage(storage)

  await reg.setOverride('cmd.alpha', 'ctrl+shift+a')
  await reg.clearOverride('cmd.alpha')

  assert.equal(reg.getOverride('cmd.alpha'), undefined)

  const alpha = reg.getAllBindings().find(r => r.commandId === 'cmd.alpha')
  assert.ok(alpha)
  assert.equal(alpha!.current, 'ctrl+a')
  assert.equal(alpha!.overridden, false)
})

test('match() honours overrides over the manifest default', async () => {
  const storage = memoryStorage()
  const reg = freshRegistryWithStorage(storage)

  // node:test runs in plain Node; KeyboardEvent isn't defined there
  // (jsdom only loads in browser-target builds). Construct a minimal
  // duck-typed shim — `match()` only reads .key and the modifier flags.
  function fakeEvent(init: {
    key: string
    ctrlKey?: boolean
    shiftKey?: boolean
    altKey?: boolean
    metaKey?: boolean
  }): KeyboardEvent {
    return {
      key: init.key,
      ctrlKey: !!init.ctrlKey,
      shiftKey: !!init.shiftKey,
      altKey: !!init.altKey,
      metaKey: !!init.metaKey,
    } as KeyboardEvent
  }

  // Default Ctrl+A maps to cmd.alpha.
  assert.equal(reg.match(fakeEvent({ key: 'a', ctrlKey: true }), {}), 'cmd.alpha')

  // After overriding to Ctrl+Shift+A, plain Ctrl+A no longer matches…
  await reg.setOverride('cmd.alpha', 'ctrl+shift+a')
  assert.equal(reg.match(fakeEvent({ key: 'a', ctrlKey: true }), {}), null)

  // …but the new chord does.
  assert.equal(
    reg.match(fakeEvent({ key: 'a', ctrlKey: true, shiftKey: true }), {}),
    'cmd.alpha',
  )
})

test('persistence round-trip: setOverride → fresh registry → loadOverrides', async () => {
  const storage = memoryStorage()

  // Session 1: user sets an override via the live registry.
  const session1 = freshRegistryWithStorage(storage)
  await session1.setOverride('cmd.alpha', 'ctrl+alt+a')
  assert.equal(storage.state['cmd.alpha'], 'ctrl+alt+a')

  // Session 2: app restarts. A new registry instance gets the same
  // bindings registered from manifests, then hydrates from storage.
  const session2 = freshRegistryWithStorage(storage)
  await session2.loadOverrides()

  const alpha = session2.getAllBindings().find(r => r.commandId === 'cmd.alpha')
  assert.ok(alpha)
  assert.equal(alpha!.current, 'ctrl+alt+a')
  assert.equal(alpha!.overridden, true)
  assert.equal(session2.getOverride('cmd.alpha'), 'ctrl+alt+a')
})

test('loadOverrides applied before manifest registration also takes effect', async () => {
  const storage = memoryStorage({ 'cmd.late': 'ctrl+l' })

  const reg = new KeybindingRegistry()
  reg.bindStorage(storage)
  await reg.loadOverrides()

  // Register a binding *after* the override was loaded — the registry
  // must consult `overrides` on registration, not just on apply.
  reg.registerFromManifest('plug.late', { command: 'cmd.late', key: 'ctrl+shift+l' })

  const row = reg.getAllBindings().find(r => r.commandId === 'cmd.late')
  assert.ok(row)
  assert.equal(row!.current, 'ctrl+l')
  assert.equal(row!.default, 'ctrl+shift+l')
  assert.equal(row!.overridden, true)
})

test('setOverride normalises chord input before persisting', async () => {
  const storage = memoryStorage()
  const reg = freshRegistryWithStorage(storage)

  await reg.setOverride('cmd.alpha', 'Cmd+Shift+A')

  // ctrl/shift/alt/meta is the canonical modifier order, so cmd
  // (alias of meta) sorts after shift.
  assert.equal(storage.state['cmd.alpha'], 'shift+meta+a')
  assert.equal(reg.getOverride('cmd.alpha'), 'shift+meta+a')
})

// ─── findByCommand ───────────────────────────────────────────────────────────

test('findByCommand — returns the active (default) chord for a registered command', () => {
  const reg = freshRegistry()
  const hit = reg.findByCommand('cmd.alpha')
  assert.ok(hit)
  assert.equal(hit?.commandId, 'cmd.alpha')
  assert.equal(hit?.chord, 'ctrl+a')
  assert.equal(hit?.defaultChord, 'ctrl+a')
})

test('findByCommand — returns undefined for an unknown command', () => {
  const reg = freshRegistry()
  assert.equal(reg.findByCommand('cmd.unknown'), undefined)
})

test('findByCommand — surfaces the user override as the active chord', async () => {
  // setOverride requires a bound storage adapter; without bindStorage
  // it warns + no-ops, which is why this test originally passed
  // memoryStorage() as a positional arg (stale signature from before
  // bindStorage was extracted). Use freshRegistryWithStorage so the
  // override actually lands.
  const reg = freshRegistryWithStorage(memoryStorage())
  await reg.setOverride('cmd.alpha', 'Ctrl+Shift+A')
  const hit = reg.findByCommand('cmd.alpha')
  assert.equal(hit?.chord, 'ctrl+shift+a', 'override wins as the active chord')
  assert.equal(hit?.defaultChord, 'ctrl+a', 'default remains for revert')
})

// ─── formattedChordFor ───────────────────────────────────────────────────────

test('formattedChordFor — returns display form of the active chord', () => {
  const reg = freshRegistry()
  // Canonical form is ctrl+a; formatted form is Title-Case parts joined by +.
  assert.equal(reg.formattedChordFor('cmd.alpha'), 'Ctrl+A')
})

test('formattedChordFor — returns undefined for an unknown command', () => {
  const reg = freshRegistry()
  assert.equal(reg.formattedChordFor('cmd.missing'), undefined)
})
