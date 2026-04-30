// shell/src/plugins/nexus/editor/cm/marginSuggestions.test.ts
//
// BL-036 phase 2 — pure-logic coverage for the margin-glyph CM
// extension's resolver, decoration builder, accept-transaction
// helper, and StateField update behaviour.
//
// We exercise the StateField + effects directly via
// `EditorState.create` rather than mounting a real EditorView's DOM
// (the ViewPlugin's glyph + card layer is browser-DOM territory and
// covered by the e2e suite later). Keeping these as pure-logic
// tests means they run under node:test without happy-dom.
//
// Run:
//   node --import tsx --test \
//     shell/src/plugins/nexus/editor/cm/marginSuggestions.test.ts

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { EditorState } from '@codemirror/state'

import {
  __test__,
  buildAcceptTransaction,
  buildDecorations,
  resolveSuggestions,
  type ResolvedSuggestion,
} from './marginSuggestions.ts'
import type { Suggestion } from '../../ai/marginSuggestStore.ts'

const { marginField, setResolvedEffect, expandEffect, dropOneEffect } = __test__

function makeStoreSuggestion(overrides: Partial<Suggestion> = {}): Suggestion {
  return {
    id: 'req-1-0',
    kind: 'tighten',
    rangeFrom: 4,
    rangeTo: 9,
    original: 'quick',
    replacement: 'fast',
    message: 'shorter',
    line: 1,
    generatedFor: 1,
    ...overrides,
  }
}

function makeResolved(overrides: Partial<ResolvedSuggestion> = {}): ResolvedSuggestion {
  return {
    id: 'req-1-0',
    kind: 'tighten',
    from: 4,
    to: 9,
    original: 'quick',
    replacement: 'fast',
    message: 'shorter',
    ...overrides,
  }
}

function makeState(doc: string): EditorState {
  return EditorState.create({ doc, extensions: [marginField] })
}

// ── resolveSuggestions ──────────────────────────────────────────────────

test('resolveSuggestions: anchors valid suggestions', () => {
  const doc = 'The quick brown fox.'
  const out = resolveSuggestions(
    [makeStoreSuggestion({ rangeFrom: 4, rangeTo: 9, original: 'quick' })],
    doc,
  )
  assert.equal(out.length, 1)
  assert.equal(out[0].from, 4)
  assert.equal(out[0].to, 9)
  assert.equal(out[0].original, 'quick')
})

test('resolveSuggestions: drops entry whose live text != original (drift)', () => {
  // Doc has been edited since the pass — the slice at [4,9) is
  // now "QUICK" rather than "quick". The engine's suggestion is
  // stale and must NOT paint.
  const doc = 'The QUICK brown fox.'
  const out = resolveSuggestions(
    [makeStoreSuggestion({ rangeFrom: 4, rangeTo: 9, original: 'quick' })],
    doc,
  )
  assert.equal(out.length, 0)
})

test('resolveSuggestions: drops out-of-bounds ranges', () => {
  const doc = 'short'
  const out = resolveSuggestions(
    [
      makeStoreSuggestion({ rangeFrom: 100, rangeTo: 110, original: 'oops' }),
      makeStoreSuggestion({ id: 'b', rangeFrom: -1, rangeTo: 3, original: 'sho' }),
      makeStoreSuggestion({ id: 'c', rangeFrom: 3, rangeTo: 3, original: '' }),
    ],
    doc,
  )
  assert.equal(out.length, 0)
})

test('resolveSuggestions: preserves multiple valid suggestions in order', () => {
  const doc = 'alpha bravo charlie'
  const out = resolveSuggestions(
    [
      makeStoreSuggestion({ id: 'a', rangeFrom: 0, rangeTo: 5, original: 'alpha' }),
      makeStoreSuggestion({ id: 'b', rangeFrom: 6, rangeTo: 11, original: 'bravo' }),
      makeStoreSuggestion({ id: 'c', rangeFrom: 12, rangeTo: 19, original: 'charlie' }),
    ],
    doc,
  )
  assert.deepEqual(
    out.map((s) => s.id),
    ['a', 'b', 'c'],
  )
})

// ── buildDecorations ────────────────────────────────────────────────────

test('buildDecorations: one mark per resolved suggestion, sorted by from', () => {
  const set = buildDecorations([
    makeResolved({ id: 'b', from: 10, to: 15 }),
    makeResolved({ id: 'a', from: 0, to: 5 }),
  ])
  const ranges: Array<{ from: number; to: number }> = []
  set.between(0, 1000, (from, to) => {
    ranges.push({ from, to })
  })
  assert.deepEqual(ranges, [
    { from: 0, to: 5 },
    { from: 10, to: 15 },
  ])
})

test('buildDecorations: empty list yields empty set', () => {
  const set = buildDecorations([])
  let count = 0
  set.between(0, 100, () => {
    count += 1
  })
  assert.equal(count, 0)
})

// ── StateField update behaviour ─────────────────────────────────────────

test('StateField: setResolvedEffect replaces suggestions and clears expandedId', () => {
  let state = makeState('alpha bravo charlie')
  // Seed an expanded id then replace.
  state = state.update({
    effects: [
      setResolvedEffect.of([makeResolved({ id: 'a', from: 0, to: 5, original: 'alpha' })]),
      expandEffect.of('a'),
    ],
  }).state
  assert.equal(state.field(marginField).expandedId, 'a')

  state = state.update({
    effects: setResolvedEffect.of([
      makeResolved({ id: 'b', from: 6, to: 11, original: 'bravo' }),
    ]),
  }).state

  const f = state.field(marginField)
  assert.equal(f.suggestions.length, 1)
  assert.equal(f.suggestions[0].id, 'b')
  assert.equal(f.expandedId, null, 'a fresh pass closes the open card so it does not dangle on a removed id')
})

test('StateField: expandEffect ignores ids the field does not know about', () => {
  let state = makeState('alpha')
  state = state.update({
    effects: [
      setResolvedEffect.of([makeResolved({ id: 'a', from: 0, to: 5, original: 'alpha' })]),
      expandEffect.of('GHOST'),
    ],
  }).state
  assert.equal(state.field(marginField).expandedId, null, 'click-after-dismiss race: unknown id stays null')
})

test('StateField: dropOneEffect removes a single suggestion + clears expandedId if matched', () => {
  let state = makeState('alpha bravo')
  state = state.update({
    effects: [
      setResolvedEffect.of([
        makeResolved({ id: 'a', from: 0, to: 5, original: 'alpha' }),
        makeResolved({ id: 'b', from: 6, to: 11, original: 'bravo' }),
      ]),
      expandEffect.of('a'),
    ],
  }).state
  state = state.update({
    effects: dropOneEffect.of('a'),
  }).state
  const f = state.field(marginField)
  assert.equal(f.suggestions.length, 1)
  assert.equal(f.suggestions[0].id, 'b')
  assert.equal(f.expandedId, null)
})

test('StateField: dropOneEffect for non-matching id is a no-op', () => {
  let state = makeState('alpha')
  state = state.update({
    effects: setResolvedEffect.of([makeResolved({ id: 'a', from: 0, to: 5, original: 'alpha' })]),
  }).state
  state = state.update({ effects: dropOneEffect.of('does-not-exist') }).state
  assert.equal(state.field(marginField).suggestions.length, 1)
})

test('StateField: doc edit OUTSIDE a suggestion preserves it (mapped + still matches)', () => {
  // Suggestion covers "quick" in "The quick brown fox.". Inserting
  // text BEFORE the span shifts the offsets but the live slice
  // still matches — must survive.
  let state = makeState('The quick brown fox.')
  state = state.update({
    effects: setResolvedEffect.of([
      makeResolved({ from: 4, to: 9, original: 'quick' }),
    ]),
  }).state
  // Insert "very " at position 0 — shifts everything right by 5.
  state = state.update({
    changes: { from: 0, to: 0, insert: 'very ' },
  }).state
  const f = state.field(marginField)
  assert.equal(f.suggestions.length, 1)
  assert.equal(f.suggestions[0].from, 9)
  assert.equal(f.suggestions[0].to, 14)
  assert.equal(state.doc.sliceString(f.suggestions[0].from, f.suggestions[0].to), 'quick')
})

test('StateField: doc edit INSIDE a suggestion drops it (live text no longer matches)', () => {
  let state = makeState('The quick brown fox.')
  state = state.update({
    effects: setResolvedEffect.of([
      makeResolved({ from: 4, to: 9, original: 'quick' }),
    ]),
  }).state
  // Replace "quick" with "QUICK" — same length, different text.
  state = state.update({
    changes: { from: 4, to: 9, insert: 'QUICK' },
  }).state
  assert.equal(
    state.field(marginField).suggestions.length,
    0,
    'drift: editing inside the span invalidates the suggestion',
  )
})

test('StateField: doc edit collapses an open card (typing during review)', () => {
  let state = makeState('The quick brown fox.')
  state = state.update({
    effects: [
      setResolvedEffect.of([makeResolved({ from: 4, to: 9, original: 'quick' })]),
      expandEffect.of('req-1-0'),
    ],
  }).state
  assert.equal(state.field(marginField).expandedId, 'req-1-0')
  // Type a space at end of doc — far from the suggestion, so the
  // suggestion survives, but the card should still close.
  state = state.update({ changes: { from: 20, to: 20, insert: ' ' } }).state
  const f = state.field(marginField)
  assert.equal(f.suggestions.length, 1, 'suggestion outside the edit survives')
  assert.equal(f.expandedId, null, 'any doc edit collapses the open card')
})

// ── buildAcceptTransaction ──────────────────────────────────────────────

test('buildAcceptTransaction: rephrase/tighten emits a replace + drop effect', () => {
  const spec = buildAcceptTransaction(
    makeResolved({
      id: 'sugg-1',
      kind: 'rephrase',
      from: 4,
      to: 9,
      replacement: 'fast',
    }),
  )
  assert.ok(spec, 'rephrase has a replacement → spec must exist')
  assert.deepEqual(spec.changes, { from: 4, to: 9, insert: 'fast' })
  // The dropOneEffect carries the suggestion id so the glyph
  // disappears in the same transaction as the doc edit.
  assert.equal(spec.effects.value, 'sugg-1')
})

test('buildAcceptTransaction: fact-check (replacement=null) returns null', () => {
  // Annotation-only suggestion — Accept on the card just dismisses;
  // no doc edit. The view-side handler dispatches a dropOneEffect
  // separately for that case.
  const spec = buildAcceptTransaction(
    makeResolved({ kind: 'fact-check', replacement: null }),
  )
  assert.equal(spec, null)
})
