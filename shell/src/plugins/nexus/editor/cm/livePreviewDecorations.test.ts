// Unit tests for `buildLivePreviewDecorations`.
//
// Builds an EditorState with the markdown language extension, runs
// the decoration builder, then introspects the returned DecorationSet
// via its rangeset cursor. We compare structural shapes (range +
// { class, replace, line }) rather than snapshot strings so the tests
// stay legible when CM internals churn.
//
// Run via the shell test runner: `pnpm --filter nexus-shell test`
// (picked up through the `tests/live-preview-decorations.test.ts`
// re-export shim).

import { EditorState, EditorSelection } from '@codemirror/state'
import { markdown } from '@codemirror/lang-markdown'
import type { DecorationSet } from '@codemirror/view'
import { buildLivePreviewDecorations } from './livePreviewDecorations.ts'

import { test } from 'node:test'
import assert from 'node:assert/strict'

interface Item {
  from: number
  to: number
  /** present for `Decoration.mark`. */
  cls?: string
  /** `true` when the deco hides the range (`Decoration.replace({})`). */
  replace?: boolean
  /** present for `Decoration.line` — class on the wrapping line. */
  line?: string
}

function decosFor(doc: string, selection?: EditorSelection): Item[] {
  const state = EditorState.create({
    doc,
    selection: selection ?? EditorSelection.cursor(0),
    // Multi-cursor tests rely on the facet being on; the default is
    // off so EditorState.create collapses extra ranges otherwise.
    extensions: [EditorState.allowMultipleSelections.of(true), markdown()],
  })
  const set: DecorationSet = buildLivePreviewDecorations(state)
  const items: Item[] = []
  const cur = set.iter()
  while (cur.value) {
    const spec = cur.value.spec as { class?: string; widget?: unknown }
    const startSide = (cur.value as unknown as { startSide?: number }).startSide
    // Heuristics over CM internals (no public `kind` field):
    //   - Decoration.line sets startSide < 0 and a class spec
    //   - Decoration.replace gives `Replace`-shaped specs (class undefined)
    //   - Decoration.mark exposes a class spec on a `Mark`-shaped value
    const isLine = startSide !== undefined && startSide < 0 && spec.class !== undefined
    const isReplace =
      cur.value.spec &&
      'inclusive' in (cur.value.spec as Record<string, unknown>) === false &&
      spec.class === undefined
    if (isLine) {
      items.push({ from: cur.from, to: cur.to, line: spec.class })
    } else if (isReplace && cur.from !== cur.to) {
      items.push({ from: cur.from, to: cur.to, replace: true })
    } else if (spec.class !== undefined) {
      items.push({ from: cur.from, to: cur.to, cls: spec.class })
    } else {
      items.push({ from: cur.from, to: cur.to, replace: true })
    }
    cur.next()
  }
  return items
}

function hasReplace(items: Item[], from: number, to: number): boolean {
  return items.some((i) => i.replace && i.from === from && i.to === to)
}

function hasMark(items: Item[], cls: string, from?: number, to?: number): boolean {
  return items.some(
    (i) =>
      i.cls === cls &&
      (from === undefined || i.from === from) &&
      (to === undefined || i.to === to),
  )
}

function hasLine(items: Item[], cls: string, from: number): boolean {
  return items.some((i) => i.line === cls && i.from === from && i.to === from)
}

// ── 1. Empty doc ──────────────────────────────────────────────────────────

test('livePreviewDecorations: empty doc → empty set', () => {
  const items = decosFor('')
  assert.equal(items.length, 0)
})

// ── 2. **bold** with cursor on the line — marks visible ───────────────────

test('livePreviewDecorations: **bold** with cursor on line keeps marks visible', () => {
  const doc = '**bold**'
  // cursor at end of line 1
  const items = decosFor(doc, EditorSelection.cursor(doc.length))
  // No replace ranges over the `**` markers when cursor is on the line.
  assert.equal(
    items.some((i) => i.replace),
    false,
    'no replace ranges when on-cursor',
  )
  // `cm-md-strong` mark over the inner `bold` text.
  assert.ok(hasMark(items, 'cm-md-strong', 2, 6), 'strong mark over inner text')
})

// ── 3. **bold** off-cursor (cursor on a different line) → marks hidden ────

test('livePreviewDecorations: **bold** off-cursor hides the ** markers', () => {
  const doc = '**bold**\nplain'
  // cursor on line 2
  const items = decosFor(doc, EditorSelection.cursor(doc.length))
  assert.ok(hasReplace(items, 0, 2), 'leading ** replaced')
  assert.ok(hasReplace(items, 6, 8), 'trailing ** replaced')
  assert.ok(hasMark(items, 'cm-md-strong', 2, 6), 'strong mark over inner text')
})

// ── 4. # Heading with cursor on the heading line ──────────────────────────

test('livePreviewDecorations: # Heading cursor-on shows the # marker', () => {
  const doc = '# Heading'
  const items = decosFor(doc, EditorSelection.cursor(0))
  assert.ok(hasLine(items, 'cm-md-h1', 0), 'h1 line decoration applied')
  assert.equal(
    items.some((i) => i.replace),
    false,
    'no replace ranges when cursor is on the heading line',
  )
})

// ── 5. # Heading with cursor on a different line ──────────────────────────

test('livePreviewDecorations: # Heading off-cursor hides the # marker', () => {
  const doc = '# Heading\nbody'
  // cursor on line 2
  const items = decosFor(doc, EditorSelection.cursor(doc.length))
  assert.ok(hasLine(items, 'cm-md-h1', 0), 'h1 line decoration applied')
  // `# ` (mark + trailing space) is replaced.
  assert.ok(
    items.some((i) => i.replace && i.from === 0 && i.to >= 1 && i.to <= 2),
    'leading "# " replaced',
  )
})

// ── 6. [text](url) off-cursor → []() hidden, link mark on text ────────────

test('livePreviewDecorations: link off-cursor hides brackets/url but marks text', () => {
  const doc = '[text](https://example.com)\nplain'
  const items = decosFor(doc, EditorSelection.cursor(doc.length))
  // Leading `[`
  assert.ok(hasReplace(items, 0, 1), 'leading [ replaced')
  // Trailing `](https://example.com)` — replace from `]` (idx 5) to `)` (idx 26)
  // Tolerate the upper bound being inclusive of trailing chars; just
  // assert there's a replace that starts at 5 and runs through end.
  assert.ok(
    items.some((i) => i.replace && i.from === 5 && i.to >= 26),
    'trailing ](url) span replaced',
  )
  assert.ok(hasMark(items, 'cm-md-link', 1, 5), 'link mark over visible text')
})

// ── 7. Setext heading: title + === underline ──────────────────────────────

test('livePreviewDecorations: setext h1 off-cursor hides the underline row', () => {
  const doc = 'Heading\n=======\nbody'
  // cursor on line 3 ("body")
  const items = decosFor(doc, EditorSelection.cursor(doc.length))
  // Title line gets the heading line decoration.
  assert.ok(hasLine(items, 'cm-md-h1', 0), 'title line gets cm-md-h1')
  // The `=======` underline row is hidden via a replace covering it
  // (and the trailing newline immediately before it).
  assert.ok(
    items.some(
      (i) => i.replace && i.from <= 7 && i.to >= 8 + '======='.length - 1,
    ),
    'underline row replaced',
  )
})

test('livePreviewDecorations: setext h1 cursor-on-title reveals both rows', () => {
  const doc = 'Heading\n=======\nbody'
  // cursor on line 1 (title)
  const items = decosFor(doc, EditorSelection.cursor(0))
  // No replace over the underline when revealed.
  // The underline starts at offset 8 (`Heading\n` length), runs to 15.
  assert.equal(
    items.some((i) => i.replace && i.from >= 7 && i.to <= 15),
    false,
    'no replace over the underline when on-cursor',
  )
  // Both rows get the heading line decoration so the visible setext
  // "row" stays visually consistent with the rendered heading.
  assert.ok(hasLine(items, 'cm-md-h1', 0), 'title line gets cm-md-h1')
  assert.ok(hasLine(items, 'cm-md-h1', 8), 'underline line gets cm-md-h1 too')
})

// ── 8. Multi-cursor: only intersected lines get revealed ──────────────────

test('livePreviewDecorations: multi-cursor reveals each cursor line, hides others', () => {
  // Three lines, each with bold; cursors on lines 1 and 3.
  const line1 = '**a**'
  const line2 = '**b**'
  const line3 = '**c**'
  const doc = `${line1}\n${line2}\n${line3}`
  const sel = EditorSelection.create([
    EditorSelection.cursor(0), // line 1
    EditorSelection.cursor(line1.length + 1 + line2.length + 1), // start of line 3
  ])
  const items = decosFor(doc, sel)
  // Line 1 (`**a**`) — no replace over its `**` markers.
  assert.equal(
    items.some((i) => i.replace && i.from === 0 && i.to === 2),
    false,
    'line 1 leading ** stays visible',
  )
  // Line 2 (`**b**`) — markers hidden.
  const l2Start = line1.length + 1 // after first `\n`
  assert.ok(
    items.some((i) => i.replace && i.from === l2Start && i.to === l2Start + 2),
    'line 2 leading ** hidden',
  )
  assert.ok(
    items.some(
      (i) => i.replace && i.from === l2Start + 3 && i.to === l2Start + 5,
    ),
    'line 2 trailing ** hidden',
  )
  // Line 3 — no replace over its markers.
  const l3Start = l2Start + line2.length + 1
  assert.equal(
    items.some((i) => i.replace && i.from === l3Start && i.to === l3Start + 2),
    false,
    'line 3 leading ** stays visible',
  )
})
