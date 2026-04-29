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
  suggestionField,
  extractPhrase,
  isInSkipZone,
  basenameNoExt,
  buildReplacement,
} = __test__

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
      from: 4,
      to: 11,
      phrase: 'foo bar',
      replacement: '[[Foo Bar|foo bar]]',
      requestId: 1,
    }),
  })
  const sug = tr.state.field(suggestionField)
  assert.ok(sug)
  assert.equal(sug?.replacement, '[[Foo Bar|foo bar]]')
})

test('a doc change invalidates the suggestion', () => {
  let state = makeState('see foo bar')
  state = state.update({
    effects: setSuggestion.of({
      from: 4,
      to: 11,
      phrase: 'foo bar',
      replacement: '[[Foo Bar|foo bar]]',
      requestId: 1,
    }),
  }).state
  state = state.update({ changes: { from: 11, to: 11, insert: 'X' } }).state
  assert.equal(state.field(suggestionField), null)
})

test('a selection move invalidates the suggestion', () => {
  let state = makeState('see foo bar')
  state = state.update({
    effects: setSuggestion.of({
      from: 4,
      to: 11,
      phrase: 'foo bar',
      replacement: '[[Foo Bar|foo bar]]',
      requestId: 1,
    }),
  }).state
  state = state.update({ selection: EditorSelection.cursor(0) }).state
  assert.equal(state.field(suggestionField), null)
})

test('explicit setSuggestion(null) clears the field', () => {
  let state = makeState('see foo bar')
  state = state.update({
    effects: setSuggestion.of({
      from: 4,
      to: 11,
      phrase: 'foo bar',
      replacement: '[[Foo|foo bar]]',
      requestId: 9,
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
  const sug = {
    from: 4,
    to: 11,
    phrase: 'foo bar',
    replacement: '[[Foo Bar|foo bar]]',
    requestId: 1,
  }
  state = state.update({ effects: setSuggestion.of(sug) }).state
  state = state.update({
    changes: { from: sug.from, to: sug.to, insert: sug.replacement },
    selection: EditorSelection.cursor(sug.from + sug.replacement.length),
  }).state
  assert.equal(state.doc.toString(), 'see [[Foo Bar|foo bar]]')
  assert.equal(state.selection.main.head, 'see [[Foo Bar|foo bar]]'.length)
})
