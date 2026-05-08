// shell/src/plugins/catalog.test.ts
//
// BL-052 follow-up — unit tests for the catalog migration helper.
// `buildLegacyIdAliases` is the load-bearing piece behind the
// transparent `nexus.activityTimeline → nexus.activity` rename: a
// user's `plugins.enabled` list gets remapped at boot if any id in
// the list matches an entry's `legacyPluginIds`.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  ALL_PLUGINS,
  buildLegacyIdAliases,
  type PluginEntry,
} from './catalog.ts'

function entry(over: Partial<PluginEntry>): PluginEntry {
  return {
    id: 'test.plugin',
    name: 'Test',
    version: '0.0.0',
    core: false,
    activationEvents: [],
    description: '',
    load: () => Promise.reject(new Error('fixture')),
    ...over,
  }
}

test('buildLegacyIdAliases: empty entries → empty map', () => {
  assert.deepEqual(buildLegacyIdAliases([]), {})
})

test('buildLegacyIdAliases: entries without legacyPluginIds → empty map', () => {
  const entries = [entry({ id: 'a' }), entry({ id: 'b' })]
  assert.deepEqual(buildLegacyIdAliases(entries), {})
})

test('buildLegacyIdAliases: each legacy id maps to its canonical id', () => {
  const entries = [
    entry({ id: 'nexus.activity', legacyPluginIds: ['nexus.activityTimeline'] }),
    entry({ id: 'nexus.foo', legacyPluginIds: ['nexus.oldFoo', 'nexus.olderFoo'] }),
  ]
  assert.deepEqual(buildLegacyIdAliases(entries), {
    'nexus.activityTimeline': 'nexus.activity',
    'nexus.oldFoo':           'nexus.foo',
    'nexus.olderFoo':         'nexus.foo',
  })
})

test('buildLegacyIdAliases: throws when two entries claim the same legacy id', () => {
  const entries = [
    entry({ id: 'a', legacyPluginIds: ['shared'] }),
    entry({ id: 'b', legacyPluginIds: ['shared'] }),
  ]
  assert.throws(
    () => buildLegacyIdAliases(entries),
    /legacy id 'shared' is claimed by both 'a' and 'b'/,
  )
})

test('buildLegacyIdAliases: throws when legacy id equals canonical id', () => {
  const entries = [entry({ id: 'self', legacyPluginIds: ['self'] })]
  assert.throws(
    () => buildLegacyIdAliases(entries),
    /must differ from the canonical id/,
  )
})

test('buildLegacyIdAliases: idempotent — re-declaring the same alias does not throw', () => {
  // Two entries with `id: 'a'` and identical legacyPluginIds is
  // technically a duplicate canonical-id (a different invariant the
  // catalog enforces elsewhere), but the alias map itself should
  // stay consistent if the same legacy id is declared twice with
  // the same target.
  const entries = [
    entry({ id: 'a', legacyPluginIds: ['oldA'] }),
    entry({ id: 'a', legacyPluginIds: ['oldA'] }),
  ]
  assert.deepEqual(buildLegacyIdAliases(entries), { oldA: 'a' })
})

test('catalog: ALL_PLUGINS round-trips through buildLegacyIdAliases without throwing', () => {
  // Sanity: the actual shipped catalog has no conflicting aliases.
  // Failing here means a future rename declared a legacy id that
  // collides with another entry's canonical or legacy.
  const aliases = buildLegacyIdAliases(ALL_PLUGINS)
  // Spot-check the BL-052 rename specifically.
  assert.equal(aliases['nexus.activityTimeline'], 'nexus.activity')
})

test('catalog: nexus.activity entry exists and declares the legacy id', () => {
  const entry = ALL_PLUGINS.find((e) => e.id === 'nexus.activity')
  assert.ok(entry, 'nexus.activity entry must be present in the catalog')
  assert.deepEqual(entry?.legacyPluginIds, ['nexus.activityTimeline'])
})
