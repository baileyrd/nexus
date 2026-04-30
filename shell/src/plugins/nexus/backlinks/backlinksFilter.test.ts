// BL-049 phase 4 — pure-logic tests for the backlinks store's
// blockFilter mode. Re-exported via
// `shell/tests/backlinks-filter.test.ts`.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { useBacklinksStore } from './backlinksStore.ts'

const A_UUID = 'd8e9f0a1-2b3c-4d5e-9f01-abcdef012345'

function reset() {
  useBacklinksStore.getState().clear()
}

test('blockFilter: defaults to null', () => {
  reset()
  assert.equal(useBacklinksStore.getState().blockFilter, null)
})

test('blockFilter: setBlockFilter writes through and round-trips', () => {
  reset()
  useBacklinksStore.getState().setBlockFilter(A_UUID)
  assert.equal(useBacklinksStore.getState().blockFilter, A_UUID)
  useBacklinksStore.getState().setBlockFilter(null)
  assert.equal(useBacklinksStore.getState().blockFilter, null)
})

test('blockFilter: clear() resets the filter alongside everything else', () => {
  reset()
  const s = useBacklinksStore.getState()
  s.setCurrent('Notes/A.md')
  s.setBlockFilter(A_UUID)
  s.setLinks([
    {
      sourceRelpath: 'Notes/B.md',
      sourceName: 'B.md',
      linkText: 'see',
      linkType: 'wikilink',
      fragment: `^${A_UUID}`,
    },
  ])

  useBacklinksStore.getState().clear()

  const after = useBacklinksStore.getState()
  assert.equal(after.blockFilter, null)
  assert.equal(after.currentRelpath, null)
  assert.deepEqual(after.links, [])
})

test('blockFilter: subscribe sees previous + next state when toggling', () => {
  reset()
  const transitions: Array<{ prev: string | null; next: string | null }> = []
  const unsub = useBacklinksStore.subscribe((state, prev) => {
    if (state.blockFilter !== prev.blockFilter) {
      transitions.push({ prev: prev.blockFilter, next: state.blockFilter })
    }
  })

  useBacklinksStore.getState().setBlockFilter(A_UUID)
  useBacklinksStore.getState().setBlockFilter(null)

  unsub()
  assert.deepEqual(transitions, [
    { prev: null, next: A_UUID },
    { prev: A_UUID, next: null },
  ])
})
