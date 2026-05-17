// BL-129 follow-up — pure-helper tests for the dream-cycle subscriber.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  composeToast,
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
