// shell/src/plugins/nexus/ai/marginSuggestStore.test.ts
//
// BL-036 phase 1 — store transitions for the AMB margin-suggestions
// engine. Mirrors the `cmdIStore.test.ts` shape (sibling file,
// node:test).
//
// Run:
//   node --import tsx --test \
//     shell/src/plugins/nexus/ai/marginSuggestStore.test.ts

import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  useMarginSuggestStore,
  type Suggestion,
} from './marginSuggestStore.ts'

function reset(): void {
  useMarginSuggestStore.getState().clear()
}

function makeSuggestion(overrides: Partial<Suggestion> = {}): Suggestion {
  return {
    id: 'req-1-0',
    kind: 'tighten',
    rangeFrom: 0,
    rangeTo: 5,
    original: 'hello',
    replacement: 'hi',
    message: 'tighter',
    line: 1,
    generatedFor: 1,
    ...overrides,
  }
}

test('beginPass: flips to pending and clears prior pass state', () => {
  reset()
  // Seed prior pass state so we can prove `beginPass` wipes it.
  useMarginSuggestStore.setState({
    status: 'done',
    suggestions: [makeSuggestion()],
    lastError: new Error('prior'),
  })

  useMarginSuggestStore.getState().beginPass('req-2', 'note.md', 7)

  const s = useMarginSuggestStore.getState()
  assert.equal(s.status, 'pending')
  assert.deepEqual(s.suggestions, [], 'prior suggestions must clear so the gutter does not paint stale glyphs')
  assert.equal(s.currentDocPath, 'note.md')
  assert.equal(s.currentGeneration, 7)
  assert.equal(s.currentRequestId, 'req-2')
  assert.equal(s.lastError, null)
})

test('setSuggestions: records list and clears request id (matching id)', () => {
  reset()
  useMarginSuggestStore.getState().beginPass('req-3', 'note.md', 1)
  const list = [makeSuggestion({ id: 'req-3-0' }), makeSuggestion({ id: 'req-3-1', rangeFrom: 10, rangeTo: 13, original: 'foo' })]

  useMarginSuggestStore.getState().setSuggestions('req-3', list)

  const s = useMarginSuggestStore.getState()
  assert.equal(s.status, 'done')
  assert.equal(s.suggestions.length, 2)
  assert.equal(s.currentRequestId, null, 'request id must clear so a tail result for the same pass is dropped')
})

test('setSuggestions: drops result from a stale request id', () => {
  reset()
  useMarginSuggestStore.getState().beginPass('req-4', 'note.md', 1)
  // Supersede with a fresh pass.
  useMarginSuggestStore.getState().beginPass('req-5', 'note.md', 2)

  useMarginSuggestStore.getState().setSuggestions('req-4', [makeSuggestion()])

  const s = useMarginSuggestStore.getState()
  assert.equal(s.status, 'pending', 'stale result must not flip to done')
  assert.equal(s.suggestions.length, 0)
  assert.equal(s.currentRequestId, 'req-5')
})

test('setError: records error and clears request id (matching id)', () => {
  reset()
  useMarginSuggestStore.getState().beginPass('req-6', 'note.md', 1)
  const err = new Error('boom')

  useMarginSuggestStore.getState().setError('req-6', err)

  const s = useMarginSuggestStore.getState()
  assert.equal(s.status, 'error')
  assert.equal(s.lastError, err)
  assert.equal(s.currentRequestId, null)
})

test('setError: drops error from a stale request id', () => {
  reset()
  useMarginSuggestStore.getState().beginPass('req-7', 'note.md', 1)
  useMarginSuggestStore.getState().beginPass('req-8', 'note.md', 2)
  const err = new Error('boom-stale')

  useMarginSuggestStore.getState().setError('req-7', err)

  const s = useMarginSuggestStore.getState()
  assert.equal(s.status, 'pending')
  assert.equal(s.lastError, null)
})

test('dismiss: removes one suggestion and leaves the rest', () => {
  reset()
  const a = makeSuggestion({ id: 'a' })
  const b = makeSuggestion({ id: 'b' })
  const c = makeSuggestion({ id: 'c' })
  useMarginSuggestStore.setState({ suggestions: [a, b, c], status: 'done' })

  useMarginSuggestStore.getState().dismiss('b')

  const s = useMarginSuggestStore.getState()
  assert.deepEqual(s.suggestions.map((x) => x.id), ['a', 'c'])
  assert.equal(s.status, 'done', 'dismiss must not change status')
})

test('dismiss: idempotent for an unknown id', () => {
  reset()
  const a = makeSuggestion({ id: 'a' })
  useMarginSuggestStore.setState({ suggestions: [a], status: 'done' })

  useMarginSuggestStore.getState().dismiss('does-not-exist')

  assert.deepEqual(
    useMarginSuggestStore.getState().suggestions.map((x) => x.id),
    ['a'],
  )
})

test('accept: removes one suggestion (phase 1: identical to dismiss)', () => {
  reset()
  const a = makeSuggestion({ id: 'a' })
  const b = makeSuggestion({ id: 'b' })
  useMarginSuggestStore.setState({ suggestions: [a, b], status: 'done' })

  useMarginSuggestStore.getState().accept('a')

  assert.deepEqual(
    useMarginSuggestStore.getState().suggestions.map((x) => x.id),
    ['b'],
  )
})

test('clear: resets every field to initial', () => {
  reset()
  useMarginSuggestStore.setState({
    status: 'done',
    suggestions: [makeSuggestion()],
    currentDocPath: 'note.md',
    currentGeneration: 9,
    currentRequestId: 'req-x',
    lastError: new Error('x'),
  })

  useMarginSuggestStore.getState().clear()

  const s = useMarginSuggestStore.getState()
  assert.equal(s.status, 'idle')
  assert.deepEqual(s.suggestions, [])
  assert.equal(s.currentDocPath, null)
  assert.equal(s.currentGeneration, 0)
  assert.equal(s.currentRequestId, null)
  assert.equal(s.lastError, null)
})
