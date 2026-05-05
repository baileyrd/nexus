// shell/src/plugins/nexus/recall/highlightRuns.test.ts
//
// AIG-06 — query-term run splitter used by the recall preview pane.
// Pure function; tested in isolation from React.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { highlightRuns } from './RecallOverlay.tsx'

test('empty query returns the whole text as a single non-matching run', () => {
  const runs = highlightRuns('hello world', '')
  assert.deepEqual(runs, [{ text: 'hello world', match: false }])
})

test('whitespace-only query returns the whole text as a single non-matching run', () => {
  const runs = highlightRuns('hello world', '   \t\n')
  assert.deepEqual(runs, [{ text: 'hello world', match: false }])
})

test('single term highlights every case-insensitive occurrence', () => {
  const runs = highlightRuns('Hello hello HELLO!', 'hello')
  // Three matches separated by single-space non-matching runs and a
  // trailing "!" non-matching run.
  const matches = runs.filter((r) => r.match).map((r) => r.text)
  assert.deepEqual(matches, ['Hello', 'hello', 'HELLO'])
})

test('multiple terms match disjunctively', () => {
  const runs = highlightRuns('the quick brown fox', 'quick fox')
  const matches = runs.filter((r) => r.match).map((r) => r.text)
  assert.deepEqual(matches, ['quick', 'fox'])
})

test('regex metacharacters in the query are escaped (no regex injection)', () => {
  // Querying for `.*` must not act as a wildcard "match everything".
  // A target string with no literal `.*` should produce zero
  // matches; a target with the literal substring should match
  // exactly once.
  const noLiteral = highlightRuns('a*b a.b', '.*')
  assert.equal(
    noLiteral.filter((r) => r.match).length,
    0,
    'metachars must not act as a wildcard',
  )
  const withLiteral = highlightRuns('the .* operator', '.*')
  const matches = withLiteral.filter((r) => r.match).map((r) => r.text)
  assert.deepEqual(matches, ['.*'])
  // Round-trip preserves the original.
  assert.equal(withLiteral.map((r) => r.text).join(''), 'the .* operator')
})

test('runs concatenate to the original input', () => {
  const inputs: Array<[string, string]> = [
    ['', 'foo'],
    ['hello world', 'world'],
    ['foo bar baz', 'bar foo'],
    ['no match here', 'xyzzy'],
  ]
  for (const [text, query] of inputs) {
    const runs = highlightRuns(text, query)
    const reconstructed = runs.map((r) => r.text).join('')
    assert.equal(reconstructed, text, `round-trip failed for (${JSON.stringify(text)}, ${JSON.stringify(query)})`)
  }
})
