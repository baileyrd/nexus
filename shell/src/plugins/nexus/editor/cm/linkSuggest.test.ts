// BL-039 — unit tests for the CodeMirror link-suggestion extension.
//
// Covers the pure helpers (extractPhrase, isInSkipZone,
// basenameNoExt, buildReplacement) and the state-machine behaviour
// of the suggestion field (set / invalidate-on-edit / accept).
//
// Tab-acceptance / Esc-dismissal at the view level need a real DOM
// EditorView; their reducers are exercised here through the
// `__test__.acceptSuggestion` / `dismissSuggestion` helpers driven
// against a state with a mounted view-mock would be redundant —
// instead we drive the state field directly to verify the splice
// math the acceptor performs.

import { EditorState, EditorSelection } from '@codemirror/state'
import { __test__ } from './linkSuggest.ts'

import { test } from 'node:test'
import assert from 'node:assert/strict'

const {
  setSuggestion,
  cycleSuggestion,
  suggestionField,
  activeCandidate,
  extractPhrase,
  isInSkipZone,
  basenameNoExt,
  buildReplacement,
} = __test__

const candidate = (
  replacement: string,
  overrides: Partial<{ from: number; to: number; phrase: string; requestId: number }> = {},
) => ({
  from: overrides.from ?? 4,
  to: overrides.to ?? 11,
  phrase: overrides.phrase ?? 'foo bar',
  replacement,
  requestId: overrides.requestId ?? 1,
})

function makeState(doc: string, headPos: number = doc.length): EditorState {
  return EditorState.create({
    doc,
    selection: EditorSelection.cursor(headPos),
    extensions: [suggestionField],
  })
}

// ── basenameNoExt ────────────────────────────────────────────────────────────

test('basenameNoExt strips dir + .md extension', () => {
  assert.equal(basenameNoExt('notes/Foo Bar.md'), 'Foo Bar')
  assert.equal(basenameNoExt('Foo.md'), 'Foo')
  assert.equal(basenameNoExt('a/b/c/Topic.md'), 'Topic')
  // Defensive: no .md extension is left intact.
  assert.equal(basenameNoExt('notes/already-clean'), 'already-clean')
})

// ── buildReplacement ─────────────────────────────────────────────────────────

test('buildReplacement uses bare form when phrase matches basename (case-insensitive)', () => {
  assert.equal(buildReplacement('notes/Foo Bar.md', 'Foo Bar'), '[[Foo Bar]]')
  assert.equal(buildReplacement('notes/Foo Bar.md', 'foo bar'), '[[Foo Bar]]')
})

test('buildReplacement uses alias form when phrase differs', () => {
  assert.equal(
    buildReplacement('notes/Project Atlas.md', 'the atlas project'),
    '[[Project Atlas|the atlas project]]',
  )
})

// ── extractPhrase ────────────────────────────────────────────────────────────

test('extractPhrase pulls trailing word group up to start of line', () => {
  const doc = 'Some intro.\nbuilding a project atlas'
  const out = extractPhrase(doc, doc.length, 4, 80)
  assert.ok(out)
  assert.equal(out?.phrase, 'building a project atlas')
  assert.equal(out?.from, doc.indexOf('building'))
})

test('extractPhrase respects sentence boundary after period+space', () => {
  const doc = 'First idea. Second sentence about atlases'
  const out = extractPhrase(doc, doc.length, 4, 80)
  assert.ok(out)
  assert.equal(out?.phrase, 'Second sentence about atlases')
})

test('extractPhrase returns null when phrase is shorter than minChars', () => {
  const doc = 'hi'
  assert.equal(extractPhrase(doc, doc.length, 4, 80), null)
})

test('extractPhrase returns null when caret is mid-word', () => {
  const doc = 'midword'
  // Caret in the middle of "midword" — text[3] is 'w', a \w char.
  assert.equal(extractPhrase(doc, 3, 4, 80), null)
})

test('extractPhrase caps at maxChars', () => {
  const doc = 'a'.repeat(200) + ' tail phrase here'
  const out = extractPhrase(doc, doc.length, 4, 20)
  assert.ok(out)
  // The phrase length cannot exceed the maxChars window.
  assert.ok(out!.phrase.length <= 20)
})

test('extractPhrase trims trailing spaces but preserves the boundary', () => {
  const doc = 'project atlas '
  const out = extractPhrase(doc, doc.length, 4, 80)
  assert.ok(out)
  assert.equal(out?.phrase, 'project atlas')
})

test('extractPhrase trims leading bullet/blockquote markers', () => {
  const doc = '- some bullet item'
  const out = extractPhrase(doc, doc.length, 4, 80)
  assert.ok(out)
  assert.equal(out?.phrase, 'some bullet item')
})

test('extractPhrase rejects pure-symbol phrases (no letters)', () => {
  const doc = '12345 67'
  assert.equal(extractPhrase(doc, doc.length, 4, 80), null)
})

// ── isInSkipZone ─────────────────────────────────────────────────────────────

test('isInSkipZone returns true when caret is inside an existing wiki-link', () => {
  const doc = 'see [[Foo'
  assert.equal(isInSkipZone(doc, doc.length), true)
})

test('isInSkipZone returns false after a closed wiki-link', () => {
  const doc = 'see [[Foo]] and more text'
  assert.equal(isInSkipZone(doc, doc.length), false)
})

test('isInSkipZone returns true inside YAML frontmatter', () => {
  const doc = '---\ntitle: Hi\nstill in fm'
  assert.equal(isInSkipZone(doc, doc.length), true)
})

test('isInSkipZone returns false after frontmatter close', () => {
  const doc = '---\ntitle: Hi\n---\nbody text here'
  assert.equal(isInSkipZone(doc, doc.length), false)
})

test('isInSkipZone returns true inside a fenced code block', () => {
  const doc = 'intro\n```\nlet x = 1\nstill code'
  assert.equal(isInSkipZone(doc, doc.length), true)
})

test('isInSkipZone returns false after a fence is closed', () => {
  const doc = 'intro\n```\nlet x = 1\n```\nback to prose'
  assert.equal(isInSkipZone(doc, doc.length), false)
})

// ── suggestionField state machine ────────────────────────────────────────────

test('suggestionField starts empty', () => {
  const state = makeState('hello')
  assert.equal(state.field(suggestionField), null)
})

test('setSuggestion effect populates the field', () => {
  const state = makeState('see foo bar')
  const tr = state.update({
    effects: setSuggestion.of({
      candidates: [candidate('[[Foo Bar|foo bar]]')],
      index: 0,
    }),
  })
  const sug = tr.state.field(suggestionField)
  assert.ok(sug)
  assert.equal(activeCandidate(sug)?.replacement, '[[Foo Bar|foo bar]]')
})

test('a doc change invalidates the suggestion', () => {
  let state = makeState('see foo bar')
  state = state.update({
    effects: setSuggestion.of({
      candidates: [candidate('[[Foo Bar|foo bar]]')],
      index: 0,
    }),
  }).state
  state = state.update({ changes: { from: 11, to: 11, insert: 'X' } }).state
  assert.equal(state.field(suggestionField), null)
})

test('a selection move invalidates the suggestion', () => {
  let state = makeState('see foo bar')
  state = state.update({
    effects: setSuggestion.of({
      candidates: [candidate('[[Foo Bar|foo bar]]')],
      index: 0,
    }),
  }).state
  state = state.update({ selection: EditorSelection.cursor(0) }).state
  assert.equal(state.field(suggestionField), null)
})

test('explicit setSuggestion(null) clears the field', () => {
  let state = makeState('see foo bar')
  state = state.update({
    effects: setSuggestion.of({
      candidates: [candidate('[[Foo|foo bar]]', { requestId: 9 })],
      index: 0,
    }),
  }).state
  state = state.update({ effects: setSuggestion.of(null) }).state
  assert.equal(state.field(suggestionField), null)
})

// ── splice math: simulate Tab acceptance via a doc change ────────────────────

test('accepting a suggestion replaces the phrase with the wiki-link form', () => {
  // Drive the state directly because acceptSuggestion needs an
  // EditorView; we mirror the change set + selection it would
  // dispatch and assert the resulting doc / caret.
  const doc = 'see foo bar'
  let state = makeState(doc, doc.length)
  const c = candidate('[[Foo Bar|foo bar]]')
  state = state.update({ effects: setSuggestion.of({ candidates: [c], index: 0 }) }).state
  state = state.update({
    changes: { from: c.from, to: c.to, insert: c.replacement },
    selection: EditorSelection.cursor(c.from + c.replacement.length),
  }).state
  assert.equal(state.doc.toString(), 'see [[Foo Bar|foo bar]]')
  assert.equal(state.selection.main.head, 'see [[Foo Bar|foo bar]]'.length)
})

// ── FU-8: cycle index advance + wrap ─────────────────────────────────────────

test('cycleSuggestion advances the index 0→1→2→0 with three candidates', () => {
  let state = makeState('see foo bar')
  const candidates = [
    candidate('[[Alpha|foo bar]]'),
    candidate('[[Bravo|foo bar]]'),
    candidate('[[Charlie|foo bar]]'),
  ]
  state = state.update({ effects: setSuggestion.of({ candidates, index: 0 }) }).state
  assert.equal(activeCandidate(state.field(suggestionField))?.replacement, '[[Alpha|foo bar]]')

  state = state.update({ effects: cycleSuggestion.of() }).state
  assert.equal(activeCandidate(state.field(suggestionField))?.replacement, '[[Bravo|foo bar]]')

  state = state.update({ effects: cycleSuggestion.of() }).state
  assert.equal(activeCandidate(state.field(suggestionField))?.replacement, '[[Charlie|foo bar]]')

  state = state.update({ effects: cycleSuggestion.of() }).state
  assert.equal(
    activeCandidate(state.field(suggestionField))?.replacement,
    '[[Alpha|foo bar]]',
    'cycle wraps from the last candidate back to the first',
  )
})

test('cycleSuggestion is a no-op when there is no mounted suggestion', () => {
  let state = makeState('see foo bar')
  state = state.update({ effects: cycleSuggestion.of() }).state
  assert.equal(state.field(suggestionField), null)
})

test('a doc edit resets the cycle (clears the candidate list)', () => {
  let state = makeState('see foo bar')
  const candidates = [
    candidate('[[Alpha|foo bar]]'),
    candidate('[[Bravo|foo bar]]'),
  ]
  state = state.update({ effects: setSuggestion.of({ candidates, index: 1 }) }).state
  assert.equal(activeCandidate(state.field(suggestionField))?.replacement, '[[Bravo|foo bar]]')
  // Any non-cycle mutation flushes the cycle, mirroring the spec —
  // typing pulls a fresh ranker pass.
  state = state.update({ changes: { from: 11, to: 11, insert: 's' } }).state
  assert.equal(state.field(suggestionField), null)
})

test('accepting picks the visible candidate, not always the top-ranked', () => {
  const doc = 'see foo bar'
  let state = makeState(doc, doc.length)
  const candidates = [
    candidate('[[Alpha|foo bar]]'),
    candidate('[[Bravo|foo bar]]'),
    candidate('[[Charlie|foo bar]]'),
  ]
  // Cycle to index 2 ("Charlie"), then mirror the splice the
  // acceptor would dispatch — the user's choice must win over the
  // top-ranked Alpha.
  state = state.update({ effects: setSuggestion.of({ candidates, index: 0 }) }).state
  state = state.update({ effects: cycleSuggestion.of() }).state
  state = state.update({ effects: cycleSuggestion.of() }).state
  const visible = activeCandidate(state.field(suggestionField))!
  assert.equal(visible.replacement, '[[Charlie|foo bar]]')
  state = state.update({
    changes: { from: visible.from, to: visible.to, insert: visible.replacement },
    selection: EditorSelection.cursor(visible.from + visible.replacement.length),
  }).state
  assert.equal(state.doc.toString(), 'see [[Charlie|foo bar]]')
})
