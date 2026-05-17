// BL-129 follow-up — pure-helper tests for the dream-cycle plugin.
//
// The toast subscriber's `composeToast` (shipped earlier) plus the
// inbox plugin's approve/skip helpers (`findRelationIndex`,
// `buildUpsertPayload`) are pure functions; the surrounding kernel /
// IPC plumbing is exercised by E2E. These tests pin the projection
// shape so the inbox doesn't accidentally drop or rename fields when
// it round-trips an entity through `entity_get` → `entity_upsert`.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  buildUpsertPayload,
  composeToast,
  findRelationIndex,
  type DreamCycleProposalsPayload,
} from '../src/plugins/nexus/dreamCycle/index'

function payload(
  partial: Partial<DreamCycleProposalsPayload> = {},
): DreamCycleProposalsPayload {
  return {
    proposals_total: 0,
    entities_enriched: 0,
    merged: 0,
    review: 0,
    ...partial,
  }
}

test('composeToast: returns empty for zero proposals (suppresses toast)', () => {
  assert.equal(composeToast(payload()), '')
})

test('composeToast: singular form for exactly one', () => {
  assert.equal(
    composeToast(payload({ proposals_total: 1 })),
    '1 new relation proposal from Dream Cycle',
  )
})

test('composeToast: plural form for two or more', () => {
  assert.equal(
    composeToast(payload({ proposals_total: 7 })),
    '7 new relation proposals from Dream Cycle',
  )
})

test('composeToast: tolerates non-finite proposals_total by suppressing', () => {
  const bad = payload({ proposals_total: NaN as unknown as number })
  assert.equal(composeToast(bad), '')
})

// ── findRelationIndex ─────────────────────────────────────────────────

function entity(
  relations: Array<{ target: string; type: string; confidence: number }>,
) {
  return {
    id: 'alice',
    entity_type: 'person',
    aliases: ['Al'],
    description: 'A friend.',
    relations,
    relpath: 'entities/alice.md',
  }
}

test('findRelationIndex: locates a matching (target, type) pair', () => {
  const e = entity([
    { target: 'nexus', type: 'works_on', confidence: 0.5 },
    { target: 'bob', type: 'knows', confidence: 1.0 },
  ])
  assert.equal(findRelationIndex(e, 'bob', 'knows'), 1)
})

test('findRelationIndex: returns -1 when no relation matches', () => {
  const e = entity([{ target: 'nexus', type: 'works_on', confidence: 0.5 }])
  assert.equal(findRelationIndex(e, 'nexus', 'reports_to'), -1)
  assert.equal(findRelationIndex(e, 'carol', 'works_on'), -1)
})

test('findRelationIndex: target+type composite is matched exactly', () => {
  const e = entity([
    { target: 'nexus', type: 'works_on', confidence: 0.5 },
    { target: 'nexus', type: 'manages', confidence: 0.5 },
  ])
  assert.equal(findRelationIndex(e, 'nexus', 'manages'), 1)
})

// ── buildUpsertPayload ────────────────────────────────────────────────

test('buildUpsertPayload: round-trips id/type/aliases/description verbatim', () => {
  const e = entity([{ target: 'nexus', type: 'works_on', confidence: 0.5 }])
  const out = buildUpsertPayload(e, e.relations)
  assert.equal(out.id, 'alice')
  assert.equal(out.entity_type, 'person')
  assert.deepEqual(out.aliases, ['Al'])
  assert.equal(out.description, 'A friend.')
})

test('buildUpsertPayload: emits relations in the upsert wire shape', () => {
  const e = entity([
    { target: 'nexus', type: 'works_on', confidence: 1.0 },
  ])
  const out = buildUpsertPayload(e, e.relations) as {
    relations: Array<{ target: string; type: string; confidence: number }>
  }
  assert.deepEqual(out.relations, [
    { target: 'nexus', type: 'works_on', confidence: 1.0 },
  ])
})

test('buildUpsertPayload: drop-row transform produces an empty relations list', () => {
  const e = entity([{ target: 'nexus', type: 'works_on', confidence: 0.5 }])
  const out = buildUpsertPayload(e, []) as { relations: unknown[] }
  assert.deepEqual(out.relations, [])
})
