// src/registry/SnippetRegistry.test.ts
// OI-18 — SnippetRegistry unit tests.
//
// Surfaced to the default `pnpm test` glob via
// `tests/snippet-registry.test.ts` (re-export shim).
//
// Coverage:
//   - register() stores the snippet and is queryable via all()
//   - registerFromManifest() is idempotent for the same id
//   - unregister() removes the snippet and re-evaluates conflicts
//   - getConflicts() detects trigger collisions across plugins
//   - getConflicts() ignores snippets with unique triggers
//   - maybeEmitConflicts: plugins:snippets-conflict fires only when the
//     conflict set actually changes (dedup by signature)
//   - Conflict clears when the offending snippet is unregistered

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { SnippetRegistry } from './SnippetRegistry.ts'
import { eventBus } from '../host/EventBus.ts'

// ─── Helpers ─────────────────────────────────────────────────────────────────

function makeSnippet(
  id: string,
  trigger: string,
  pluginId = 'plugin.a',
  body = 'body',
) {
  return { id, trigger, body, pluginId }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

test('OI-18 — register() stores entry; all() returns it', () => {
  const reg = new SnippetRegistry()
  reg.register('plugin.a', { id: 'a.date', trigger: 'date', body: '{{date}}' })
  const all = reg.all()
  assert.equal(all.length, 1)
  assert.equal(all[0].trigger, 'date')
  assert.equal(all[0].pluginId, 'plugin.a')
})

test('OI-18 — registerFromManifest() is idempotent for same id', () => {
  const reg = new SnippetRegistry()
  const contrib = { id: 'a.foo', trigger: 'foo', body: 'FOO' }
  reg.registerFromManifest('plugin.a', contrib)
  reg.registerFromManifest('plugin.a', contrib) // second call is a no-op
  assert.equal(reg.all().length, 1)
})

test('OI-18 — unregister() removes the entry', () => {
  const reg = new SnippetRegistry()
  reg.register('plugin.a', { id: 'a.date', trigger: 'date', body: '{{date}}' })
  reg.unregister('a.date')
  assert.equal(reg.all().length, 0)
})

test('OI-18 — getConflicts() returns empty when all triggers are unique', () => {
  const reg = new SnippetRegistry()
  reg.register('plugin.a', { id: 'a.date', trigger: 'date', body: '{{date}}' })
  reg.register('plugin.b', { id: 'b.time', trigger: 'time', body: '{{time}}' })
  assert.deepEqual(reg.getConflicts(), [])
})

test('OI-18 — getConflicts() detects collision across two plugins', () => {
  const reg = new SnippetRegistry()
  reg.register('plugin.a', { id: 'a.date', trigger: 'date', body: '{{date}}' })
  reg.register('plugin.b', { id: 'b.date', trigger: 'date', body: '[[date]]' })
  const conflicts = reg.getConflicts()
  assert.equal(conflicts.length, 1)
  assert.equal(conflicts[0].trigger, 'date')
  assert.equal(conflicts[0].entries.length, 2)
})

test('OI-18 — getConflicts() handles multiple conflicting triggers', () => {
  const reg = new SnippetRegistry()
  reg.register('plugin.a', { id: 'a.foo', trigger: 'foo', body: 'FOO-A' })
  reg.register('plugin.b', { id: 'b.foo', trigger: 'foo', body: 'FOO-B' })
  reg.register('plugin.a', { id: 'a.bar', trigger: 'bar', body: 'BAR-A' })
  reg.register('plugin.b', { id: 'b.bar', trigger: 'bar', body: 'BAR-B' })
  assert.equal(reg.getConflicts().length, 2)
})

test('OI-18 — conflict clears when offending snippet is unregistered', () => {
  const reg = new SnippetRegistry()
  reg.register('plugin.a', { id: 'a.date', trigger: 'date', body: '{{date}}' })
  reg.register('plugin.b', { id: 'b.date', trigger: 'date', body: '[[date]]' })
  assert.equal(reg.getConflicts().length, 1)
  reg.unregister('b.date')
  assert.equal(reg.getConflicts().length, 0)
})

test('OI-18 — plugins:snippets-conflict fires on first collision', () => {
  const reg = new SnippetRegistry()
  const events: unknown[] = []
  const unsub = eventBus.on('plugins:snippets-conflict', (p) => events.push(p))
  try {
    reg.register('plugin.a', { id: 'a.x', trigger: 'x', body: 'X-A' })
    assert.equal(events.length, 0, 'no conflict yet — no event')
    reg.register('plugin.b', { id: 'b.x', trigger: 'x', body: 'X-B' })
    assert.equal(events.length, 1, 'conflict appeared — event fired')
  } finally {
    unsub()
  }
})

test('OI-18 — plugins:snippets-conflict does not re-fire when conflict set is unchanged', () => {
  const reg = new SnippetRegistry()
  const events: unknown[] = []
  const unsub = eventBus.on('plugins:snippets-conflict', (p) => events.push(p))
  try {
    reg.register('plugin.a', { id: 'a.x', trigger: 'x', body: 'X-A' })
    reg.register('plugin.b', { id: 'b.x', trigger: 'x', body: 'X-B' })
    const countAfterFirst = events.length
    // Adding a non-conflicting snippet should NOT re-fire the conflict event
    reg.register('plugin.a', { id: 'a.y', trigger: 'y', body: 'Y' })
    assert.equal(events.length, countAfterFirst, 'unchanged conflict set → no extra event')
  } finally {
    unsub()
  }
})

test('OI-18 — plugins:snippets-conflict fires again when conflict resolves', () => {
  const reg = new SnippetRegistry()
  const events: unknown[] = []
  const unsub = eventBus.on('plugins:snippets-conflict', (p) => events.push(p))
  try {
    reg.register('plugin.a', { id: 'a.x', trigger: 'x', body: 'X-A' })
    reg.register('plugin.b', { id: 'b.x', trigger: 'x', body: 'X-B' })
    const countAfterConflict = events.length
    reg.unregister('b.x') // resolve conflict
    assert.equal(events.length, countAfterConflict + 1, 'resolution fires a new event')
    const last = events[events.length - 1] as { conflicts: unknown[] }
    assert.equal(last.conflicts.length, 0, 'event payload has no conflicts')
  } finally {
    unsub()
  }
})
