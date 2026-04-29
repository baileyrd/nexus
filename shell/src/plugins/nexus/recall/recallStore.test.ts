// shell/src/plugins/nexus/recall/recallStore.test.ts
//
// BL-044 — recall store unit tests. Covers open/close/setQuery,
// selectedIndex clamping under arrow navigation, and the request-id
// guard that drops stale `setResults` callbacks.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { useRecallStore, type RecallMatch } from './recallStore.ts'

function reset(): void {
  useRecallStore.setState({
    visible: false,
    query: '',
    results: [],
    selectedIndex: 0,
    status: 'idle',
    error: null,
    currentRequestId: null,
  })
}

const FAKE_MATCHES: RecallMatch[] = [
  { file_path: 'Inbox.md', chunk_text: 'one', score: 0.9 },
  { file_path: 'Inbox.md', chunk_text: 'two', score: 0.8 },
  { file_path: 'Inbox.md', chunk_text: 'three', score: 0.7 },
]

test('open() resets all transient state and flips visible', () => {
  reset()
  useRecallStore.setState({
    query: 'stale',
    results: FAKE_MATCHES,
    selectedIndex: 2,
    status: 'error',
    error: new Error('old'),
  })
  useRecallStore.getState().open()
  const s = useRecallStore.getState()
  assert.equal(s.visible, true)
  assert.equal(s.query, '')
  assert.equal(s.results.length, 0)
  assert.equal(s.selectedIndex, 0)
  assert.equal(s.status, 'idle')
  assert.equal(s.error, null)
  assert.equal(s.currentRequestId, null)
})

test('close() flips visible but retains query/results', () => {
  reset()
  useRecallStore.getState().open()
  useRecallStore.getState().setQuery('q')
  useRecallStore.setState({ results: FAKE_MATCHES })
  useRecallStore.getState().close()
  const s = useRecallStore.getState()
  assert.equal(s.visible, false)
  assert.equal(s.query, 'q')
  assert.equal(s.results.length, 3)
  assert.equal(s.currentRequestId, null)
})

test('setQuery() updates query without touching results', () => {
  reset()
  useRecallStore.setState({ results: FAKE_MATCHES })
  useRecallStore.getState().setQuery('hello')
  const s = useRecallStore.getState()
  assert.equal(s.query, 'hello')
  assert.equal(s.results.length, 3)
})

test('beginSearch + setResults: matching id commits, mismatched id is dropped', () => {
  reset()
  useRecallStore.getState().beginSearch('req-1')
  assert.equal(useRecallStore.getState().status, 'searching')
  // Stale callback for an old request id — must NOT replace results.
  useRecallStore.getState().setResults('req-old', FAKE_MATCHES)
  assert.equal(useRecallStore.getState().results.length, 0)
  assert.equal(useRecallStore.getState().status, 'searching')
  // Matching id — commits.
  useRecallStore.getState().setResults('req-1', FAKE_MATCHES)
  assert.equal(useRecallStore.getState().results.length, 3)
  assert.equal(useRecallStore.getState().status, 'idle')
  assert.equal(useRecallStore.getState().currentRequestId, null)
})

test('moveSelection() clamps to [0, results.length-1]', () => {
  reset()
  useRecallStore.getState().beginSearch('r')
  useRecallStore.getState().setResults('r', FAKE_MATCHES)
  // Already at 0 — going up stays at 0.
  useRecallStore.getState().moveSelection(-1)
  assert.equal(useRecallStore.getState().selectedIndex, 0)
  // Down twice → index 2.
  useRecallStore.getState().moveSelection(+1)
  useRecallStore.getState().moveSelection(+1)
  assert.equal(useRecallStore.getState().selectedIndex, 2)
  // One more down — clamps at length-1.
  useRecallStore.getState().moveSelection(+1)
  assert.equal(useRecallStore.getState().selectedIndex, 2)
})

test('moveSelection() with no results pins index at 0', () => {
  reset()
  useRecallStore.getState().moveSelection(+1)
  assert.equal(useRecallStore.getState().selectedIndex, 0)
})

test('setResults() reclamps selectedIndex when the new list is shorter', () => {
  reset()
  useRecallStore.getState().beginSearch('a')
  useRecallStore.getState().setResults('a', FAKE_MATCHES)
  useRecallStore.getState().setSelectedIndex(2)
  // New search with one result.
  useRecallStore.getState().beginSearch('b')
  useRecallStore.getState().setResults('b', [FAKE_MATCHES[0]])
  assert.equal(useRecallStore.getState().selectedIndex, 0)
})

test('setError() flips status and clears the request id', () => {
  reset()
  useRecallStore.getState().beginSearch('z')
  useRecallStore.getState().setError(new Error('boom'))
  const s = useRecallStore.getState()
  assert.equal(s.status, 'error')
  assert.equal(s.error?.message, 'boom')
  assert.equal(s.currentRequestId, null)
})
