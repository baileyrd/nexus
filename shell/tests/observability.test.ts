// BL-054 Phase 4 — observability aggregator + path-classifier tests.

import { strict as assert } from 'node:assert'
import { test } from 'node:test'

import { aggregateUsage } from '../src/plugins/nexus/observability/usageAggregate'
import { isVaultPath } from '../src/plugins/nexus/observability'
import type { ActivityEntry } from '../src/plugins/nexus/activityTimeline/activityTimelineStore'

const FROZEN_NOW = new Date('2026-05-07T12:00:00Z')

function entry(over: Partial<ActivityEntry>): ActivityEntry {
  return {
    id: over.id ?? Math.random().toString(36).slice(2),
    timestamp: over.timestamp ?? '2026-05-07T10:00:00Z',
    session_id: over.session_id ?? 'sess',
    surface: over.surface ?? 'chat',
    origin: over.origin ?? 'ai',
    prompt: over.prompt ?? '',
    outcome: over.outcome ?? 'ok',
  }
}

test('aggregateUsage: empty input yields zeroed rollup with backfilled days', () => {
  const out = aggregateUsage([], 7, FROZEN_NOW)
  assert.equal(out.total, 0)
  assert.equal(out.bySurface.length, 0)
  assert.equal(out.byDay.length, 7)
  assert.equal(out.byDay[6].date, '2026-05-07')
  assert.equal(out.byDay[6].total, 0)
  assert.equal(out.latest, null)
})

test('aggregateUsage: per-surface counts split by outcome and sort by total desc', () => {
  const out = aggregateUsage(
    [
      entry({ surface: 'chat', outcome: 'ok' }),
      entry({ surface: 'chat', outcome: 'ok' }),
      entry({ surface: 'chat', outcome: 'error' }),
      entry({ surface: 'file', outcome: 'ok' }),
    ],
    14,
    FROZEN_NOW,
  )
  assert.equal(out.total, 4)
  assert.equal(out.bySurface[0].surface, 'chat')
  assert.equal(out.bySurface[0].total, 3)
  assert.equal(out.bySurface[0].ok, 2)
  assert.equal(out.bySurface[0].error, 1)
  assert.equal(out.bySurface[1].surface, 'file')
  assert.equal(out.bySurface[1].total, 1)
})

test('aggregateUsage: byDay backfill keeps zero days alongside observed counts', () => {
  const out = aggregateUsage(
    [
      entry({ timestamp: '2026-05-07T08:00:00Z', outcome: 'ok' }),
      entry({ timestamp: '2026-05-07T09:00:00Z', outcome: 'error' }),
      entry({ timestamp: '2026-05-05T10:00:00Z', outcome: 'ok' }),
    ],
    7,
    FROZEN_NOW,
  )
  const lookup = Object.fromEntries(out.byDay.map((d) => [d.date, d]))
  assert.equal(lookup['2026-05-07'].total, 2)
  assert.equal(lookup['2026-05-07'].error, 1)
  assert.equal(lookup['2026-05-06'].total, 0) // gap day kept
  assert.equal(lookup['2026-05-05'].total, 1)
})

test('aggregateUsage: tracks latest timestamp', () => {
  const out = aggregateUsage(
    [
      entry({ timestamp: '2026-05-01T00:00:00Z' }),
      entry({ timestamp: '2026-05-07T08:00:00Z' }),
      entry({ timestamp: '2026-05-03T00:00:00Z' }),
    ],
    14,
    FROZEN_NOW,
  )
  assert.equal(out.latest, '2026-05-07T08:00:00Z')
})

test('isVaultPath: matches the OS template root prefixes', () => {
  assert.equal(isVaultPath('raw/notes.md'), true)
  assert.equal(isVaultPath('wiki/concept.md'), true)
  assert.equal(isVaultPath('output/2026-05-07.md'), true)
  assert.equal(isVaultPath('projects/foo/state.md'), true)
  assert.equal(isVaultPath('ops/runbook.md'), true)
})

test('isVaultPath: rejects non-vault paths', () => {
  assert.equal(isVaultPath(''), false)
  assert.equal(isVaultPath('archive/2025/old.md'), false) // not a vault root for the feed
  assert.equal(isVaultPath('CLAUDE.md'), false)
  assert.equal(isVaultPath('.forge/sessions/foo'), false)
})

test('isVaultPath: tolerates ./ prefix and Windows separators', () => {
  assert.equal(isVaultPath('./raw/foo.md'), true)
  assert.equal(isVaultPath('raw\\foo.md'), true)
})
