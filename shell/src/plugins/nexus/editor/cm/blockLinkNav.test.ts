// Pure-logic + minimal DOM tests for the BL-049 phase-2
// block-link navigation extension. Re-exported via
// `shell/tests/block-link-nav.test.ts` so the default `pnpm test`
// glob picks them up.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { EditorSelection, EditorState } from '@codemirror/state'
import { EditorView } from '@codemirror/view'

import {
  buildBlockLinkDecorations,
  handleBlockLinkMousedown,
  revealBlockInView,
} from './blockLinkNav.ts'

const A_UUID = 'd8e9f0a1-2b3c-4d5e-9f01-abcdef012345'
const B_UUID = '11111111-2222-3333-4444-555555555555'

// ── buildBlockLinkDecorations ───────────────────────────────────────────────

test('decorations: emits one mark per block link', () => {
  const doc = `intro [[A.md#^${A_UUID}]] middle [[B.md#^${B_UUID}|alt]] end`
  const state = EditorState.create({ doc })
  const set = buildBlockLinkDecorations(state)
  const ranges: Array<{ from: number; to: number }> = []
  set.between(0, doc.length, (from, to) => ranges.push({ from, to }))
  assert.equal(ranges.length, 2)
  assert.equal(doc.slice(ranges[0].from, ranges[0].to), `[[A.md#^${A_UUID}]]`)
  assert.equal(doc.slice(ranges[1].from, ranges[1].to), `[[B.md#^${B_UUID}|alt]]`)
})

test('decorations: ignores plain wikilinks and heading anchors', () => {
  const state = EditorState.create({
    doc: 'intro [[A.md]] [[B.md#some-heading]] end',
  })
  const set = buildBlockLinkDecorations(state)
  let count = 0
  set.between(0, state.doc.length, () => {
    count++
  })
  assert.equal(count, 0)
})

// ── revealBlockInView ───────────────────────────────────────────────────────

test('revealBlockInView scrolls to the line carrying the stable-id marker', () => {
  const doc = `# Header\n\nfirst paragraph\n<!-- ^${A_UUID} -->\n\ntail\n`
  const view = new EditorView({
    state: EditorState.create({
      doc,
      selection: EditorSelection.single(0),
    }),
  })
  const ok = revealBlockInView(view, A_UUID)
  assert.equal(ok, true)
  // Selection lands at the start of the marker's line.
  const head = view.state.selection.main.head
  const lineText = view.state.doc.lineAt(head).text
  assert.match(lineText, /<!-- \^/)
  view.destroy()
})

test('revealBlockInView returns false when the marker is absent', () => {
  const view = new EditorView({
    state: EditorState.create({ doc: 'no marker here\n' }),
  })
  assert.equal(revealBlockInView(view, A_UUID), false)
  view.destroy()
})

test('revealBlockInView is case-insensitive on the UUID', () => {
  const upper = A_UUID.toUpperCase()
  const doc = `body\n<!-- ^${upper} -->\n`
  const view = new EditorView({ state: EditorState.create({ doc }) })
  // User-typed link uses lowercase; on-disk marker is upper-case.
  // The reveal must still match.
  assert.equal(revealBlockInView(view, A_UUID), true)
  view.destroy()
})

// ── handleBlockLinkMousedown ────────────────────────────────────────────────

test('plain left-click on a block-link range invokes onNavigate', () => {
  const doc = `prefix [[Notes/A.md#^${A_UUID}]] suffix\n`
  const state = EditorState.create({ doc })
  const navigated: Array<{ filePath: string; blockId: string; label: string | null }> = []
  let prevented = false
  const handled = handleBlockLinkMousedown(
    state,
    doc.indexOf('[[') + 5,
    {
      button: 0,
      preventDefault: () => {
        prevented = true
      },
    },
    {
      onNavigate: (link) =>
        navigated.push({
          filePath: link.filePath,
          blockId: link.blockId,
          label: link.label,
        }),
    },
  )
  assert.equal(handled, true)
  assert.equal(prevented, true)
  assert.equal(navigated.length, 1)
  assert.equal(navigated[0].filePath, 'Notes/A.md')
  assert.equal(navigated[0].blockId, A_UUID)
  assert.equal(navigated[0].label, null)
})

test('Mod-click / Shift-click / Alt-click / right-click fall through (chord-click semantics)', () => {
  const doc = `[[A.md#^${A_UUID}]]`
  const state = EditorState.create({ doc })
  const navigated: number[] = []
  const deps = { onNavigate: () => navigated.push(1) }
  const inLink = 4
  for (const event of [
    { button: 0, metaKey: true },
    { button: 0, ctrlKey: true },
    { button: 0, shiftKey: true },
    { button: 0, altKey: true },
    { button: 2 }, // right-click
    { button: 1 }, // middle-click
  ]) {
    const handled = handleBlockLinkMousedown(state, inLink, event, deps)
    assert.equal(handled, false, `chord ${JSON.stringify(event)} must fall through`)
  }
  assert.equal(navigated.length, 0)
})

test('clicks outside any block-link range fall through', () => {
  const doc = `prefix [[A.md#^${A_UUID}]] suffix`
  const state = EditorState.create({ doc })
  const navigated: number[] = []
  // Position 0 is well outside the link.
  const handled = handleBlockLinkMousedown(
    state,
    0,
    { button: 0 },
    { onNavigate: () => navigated.push(1) },
  )
  assert.equal(handled, false)
  assert.equal(navigated.length, 0)
})

test('parsed link carries the pipe-aliased label when present', () => {
  const doc = `[[A.md#^${A_UUID}|see this]]`
  const state = EditorState.create({ doc })
  let captured: string | null = null
  handleBlockLinkMousedown(
    state,
    3,
    { button: 0 },
    { onNavigate: (l) => (captured = l.label) },
  )
  assert.equal(captured, 'see this')
})
