// C44 (#422) — composeToast now reports relation proposals and/or newly
// extracted entities. Pure logic, no kernel mock needed.
import { test } from 'node:test'
import assert from 'node:assert/strict'

import { composeToast } from './index.ts'

test('reports relation proposals only, singular', () => {
  const msg = composeToast({ proposals_total: 1, entities_enriched: 0, merged: 0, review: 0 })
  assert.equal(msg, '1 new relation proposal from Dream Cycle')
})

test('reports relation proposals only, plural', () => {
  const msg = composeToast({ proposals_total: 3, entities_enriched: 0, merged: 0, review: 0 })
  assert.equal(msg, '3 new relation proposals from Dream Cycle')
})

test('reports extracted entities only, singular', () => {
  const msg = composeToast({
    proposals_total: 0,
    entities_enriched: 0,
    entities_extracted: 1,
    merged: 0,
    review: 0,
  })
  assert.equal(msg, '1 new entity extracted from Dream Cycle')
})

test('reports extracted entities only, plural', () => {
  const msg = composeToast({
    proposals_total: 0,
    entities_enriched: 0,
    entities_extracted: 4,
    merged: 0,
    review: 0,
  })
  assert.equal(msg, '4 new entities extracted from Dream Cycle')
})

test('reports both proposals and extracted entities together', () => {
  const msg = composeToast({
    proposals_total: 2,
    entities_enriched: 0,
    entities_extracted: 1,
    merged: 0,
    review: 0,
  })
  assert.equal(msg, '2 new relation proposals, 1 new entity extracted from Dream Cycle')
})

test('returns empty string when nothing happened', () => {
  const msg = composeToast({ proposals_total: 0, entities_enriched: 0, merged: 0, review: 0 })
  assert.equal(msg, '')
})

test('treats a missing entities_extracted field as zero (older backend payload)', () => {
  const msg = composeToast({ proposals_total: 2, entities_enriched: 0, merged: 0, review: 0 })
  assert.equal(msg, '2 new relation proposals from Dream Cycle')
})

test('treats NaN proposals_total as zero', () => {
  const msg = composeToast({
    proposals_total: Number.NaN,
    entities_enriched: 0,
    entities_extracted: 2,
    merged: 0,
    review: 0,
  })
  assert.equal(msg, '2 new entities extracted from Dream Cycle')
})
