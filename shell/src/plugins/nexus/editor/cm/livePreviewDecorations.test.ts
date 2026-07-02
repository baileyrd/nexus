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
import { Table } from '@lezer/markdown'
import { syntaxTree } from '@codemirror/language'
import type { DecorationSet } from '@codemirror/view'
import { buildLivePreviewDecorations, TableWidget, FencedCodeWidget } from './livePreviewDecorations.ts'
import { fencedCodeRegistry } from './fencedCodeRegistry.ts'

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
  /** name of the widget constructor when the replace carries one. */
  widget?: string
  /** `true` when the replace is a block-replace covering full line(s). */
  block?: boolean
}

function decosFor(doc: string, selection?: EditorSelection): Item[] {
  const state = EditorState.create({
    doc,
    selection: selection ?? EditorSelection.cursor(0),
    // Multi-cursor tests rely on the facet being on; the default is
    // off so EditorState.create collapses extra ranges otherwise.
    extensions: [
      EditorState.allowMultipleSelections.of(true),
      markdown({ extensions: [Table] }),
    ],
  })
  const set: DecorationSet = buildLivePreviewDecorations(state)
  const items: Item[] = []
  const cur = set.iter()
  while (cur.value) {
    const spec = cur.value.spec as { class?: string; widget?: unknown; block?: boolean }
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
      const widgetName = spec.widget
        ? (spec.widget as { constructor: { name: string } }).constructor.name
        : undefined
      items.push({
        from: cur.from,
        to: cur.to,
        replace: true,
        widget: widgetName,
        block: spec.block === true,
      })
    } else if (spec.class !== undefined) {
      items.push({ from: cur.from, to: cur.to, cls: spec.class })
    } else {
      const widgetName = spec.widget
        ? (spec.widget as { constructor: { name: string } }).constructor.name
        : undefined
      items.push({
        from: cur.from,
        to: cur.to,
        replace: true,
        widget: widgetName,
        block: spec.block === true,
      })
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

// ── HR widget: off-cursor → block-replace with HrWidget ───────────────────

test('livePreviewDecorations: HR off-cursor emits a block-replace HrWidget', () => {
  const doc = 'para\n\n---\n\nmore'
  // cursor on line 1 ("para")
  const items = decosFor(doc, EditorSelection.cursor(0))
  // `---` line spans offsets 6..9 (after "para\n\n").
  const hr = items.find(
    (i) => i.replace && i.from === 6 && i.to === 9 && i.widget === 'HrWidget',
  )
  assert.ok(hr, 'HR replace with HrWidget over the dash line')
  assert.equal(hr?.block, true, 'HR replace is a block decoration')
})

test('livePreviewDecorations: HR on-cursor leaves the dash line raw', () => {
  const doc = 'para\n\n---\n\nmore'
  // cursor on the `---` line (offset 6 = line start).
  const items = decosFor(doc, EditorSelection.cursor(7))
  assert.equal(
    items.some((i) => i.widget === 'HrWidget'),
    false,
    'no HrWidget when the HR line is active',
  )
  assert.equal(
    items.some((i) => i.replace && i.from === 6 && i.to === 9),
    false,
    'no replace covering the HR line on-cursor',
  )
})

// ── Fenced code: fence lines hidden off-cursor, inner kept ────────────────

test('livePreviewDecorations: fenced code off-cursor hides both fence lines', () => {
  const doc = '```js\nconst x = 1\n```'
  // Place cursor at the very end (last fence) — but we want fence-lines
  // hidden, so move cursor outside the block by appending a paragraph
  // and parking the cursor there.
  const docWithPara = `${doc}\npara`
  const items = decosFor(
    docWithPara,
    EditorSelection.cursor(docWithPara.length),
  )
  const opener = '```js'
  const inner = 'const x = 1'
  const openerFrom = 0
  const openerTo = opener.length // 5
  const innerFrom = openerTo + 1 // after \n
  const innerTo = innerFrom + inner.length
  const closerFrom = innerTo + 1 // after \n
  const closerTo = closerFrom + 3 // ```
  // Opener line replaced.
  assert.ok(
    hasReplace(items, openerFrom, openerTo),
    'opener fence line replaced',
  )
  // Closer line replaced.
  assert.ok(
    hasReplace(items, closerFrom, closerTo),
    'closer fence line replaced',
  )
  // Inner line NOT replaced.
  assert.equal(
    items.some((i) => i.replace && i.from === innerFrom && i.to === innerTo),
    false,
    'inner line not replaced',
  )
  // Line decoration `cm-md-codeblock` on all three lines.
  assert.ok(hasLine(items, 'cm-md-codeblock', openerFrom), 'opener line decoration')
  assert.ok(hasLine(items, 'cm-md-codeblock', innerFrom), 'inner line decoration')
  assert.ok(hasLine(items, 'cm-md-codeblock', closerFrom), 'closer line decoration')
})

test('livePreviewDecorations: fenced code with cursor on opener keeps opener raw', () => {
  const doc = '```js\nconst x = 1\n```'
  // Cursor on the opener line.
  const items = decosFor(doc, EditorSelection.cursor(2))
  const openerTo = '```js'.length
  const innerFrom = openerTo + 1
  const innerTo = innerFrom + 'const x = 1'.length
  const closerFrom = innerTo + 1
  const closerTo = closerFrom + 3
  // Opener NOT replaced.
  assert.equal(
    items.some((i) => i.replace && i.from === 0 && i.to === openerTo),
    false,
    'opener stays visible when cursor is on it',
  )
  // Closer still replaced.
  assert.ok(
    hasReplace(items, closerFrom, closerTo),
    'closer fence line still replaced',
  )
})

test('livePreviewDecorations: fenced code cursor on inner line replaces both fences', () => {
  const doc = '```js\nconst x = 1\n```'
  const openerTo = '```js'.length
  const innerFrom = openerTo + 1
  // Cursor on the inner line.
  const items = decosFor(doc, EditorSelection.cursor(innerFrom + 2))
  const innerTo = innerFrom + 'const x = 1'.length
  const closerFrom = innerTo + 1
  const closerTo = closerFrom + 3
  assert.ok(hasReplace(items, 0, openerTo), 'opener replaced when cursor on inner')
  assert.ok(
    hasReplace(items, closerFrom, closerTo),
    'closer replaced when cursor on inner',
  )
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

// ── GFM Table: off-cursor → block-replace TableWidget ─────────────────────

const TABLE_DOC = [
  'before',
  '',
  '| h1 | h2 |',
  '| --- | --- |',
  '| a | b |',
  '',
  'after',
].join('\n')

// Sanity: the GFM Table extension must actually wire into the syntax tree,
// otherwise the decoration walker has nothing to match on. Confirm this
// before asserting decoration shapes.
test('livePreviewDecorations: syntax tree contains Table node when GFM is enabled', () => {
  const state = EditorState.create({
    doc: TABLE_DOC,
    extensions: [markdown({ extensions: [Table] })],
  })
  let found = false
  syntaxTree(state).iterate({
    enter(n) {
      if (n.name === 'Table') found = true
    },
  })
  assert.ok(found, 'Table node present in tree')
})

test('livePreviewDecorations: GFM table off-cursor emits block-replace TableWidget', () => {
  const items = decosFor(TABLE_DOC, EditorSelection.cursor(0))
  // Table spans lines 3..5 (1-indexed); compute byte offsets from doc.
  const before = 'before\n\n'
  const tableSrc = '| h1 | h2 |\n| --- | --- |\n| a | b |'
  const tableFrom = before.length
  const tableTo = tableFrom + tableSrc.length
  const tw = items.find(
    (i) =>
      i.replace &&
      i.widget === 'TableWidget' &&
      i.from === tableFrom &&
      i.to === tableTo,
  )
  assert.ok(tw, 'block-replace TableWidget over the table line range')
  assert.equal(tw?.block, true, 'table replace is a block decoration')
})

test('livePreviewDecorations: GFM table on-cursor leaves raw pipes visible', () => {
  // Cursor inside the second table row.
  const cursor = 'before\n\n| h1 | h2 |\n'.length + 2
  const items = decosFor(TABLE_DOC, EditorSelection.cursor(cursor))
  assert.equal(
    items.some((i) => i.widget === 'TableWidget'),
    false,
    'no TableWidget when cursor is inside the table',
  )
})

test('TableWidget.eq: same source ⇒ true; different source ⇒ false', () => {
  const a = new TableWidget('| x | y |\n| - | - |\n| 1 | 2 |')
  const b = new TableWidget('| x | y |\n| - | - |\n| 1 | 2 |')
  const c = new TableWidget('| x | y |\n| - | - |\n| 3 | 4 |')
  assert.equal(a.eq(b), true)
  assert.equal(a.eq(c), false)
})

// ── BL-008: Fenced-code renderer integration ─────────────────────────────

test('livePreviewDecorations: registered language emits FencedCodeWidget when off-cursor', () => {
  const dispose = fencedCodeRegistry.register('decoTest', () => {
    return { nodeType: 1 } as unknown as HTMLElement
  })
  try {
    const doc = '```decoTest\nfoo\n```\npara'
    const items = decosFor(doc, EditorSelection.cursor(doc.length))
    const widget = items.find(
      (i) => i.replace && i.widget === 'FencedCodeWidget' && i.from === 0,
    )
    assert.ok(widget, 'block-replace FencedCodeWidget over the fenced block')
    assert.equal(widget?.block, true, 'fenced widget is a block decoration')
  } finally {
    dispose()
  }
})

test('livePreviewDecorations: registered language with cursor inside falls back to raw', () => {
  const dispose = fencedCodeRegistry.register('decoCursor', () => {
    return { nodeType: 1 } as unknown as HTMLElement
  })
  try {
    const doc = '```decoCursor\nfoo\n```'
    const innerStart = '```decoCursor\n'.length
    const items = decosFor(doc, EditorSelection.cursor(innerStart + 1))
    assert.equal(
      items.some((i) => i.widget === 'FencedCodeWidget'),
      false,
      'no fenced widget when cursor is inside the block',
    )
  } finally {
    dispose()
  }
})

test('livePreviewDecorations: unregistered language keeps existing fence-line behaviour', () => {
  const doc = '```nonesuch-bl008\nfoo\n```\npara'
  const items = decosFor(doc, EditorSelection.cursor(doc.length))
  assert.equal(
    items.some((i) => i.widget === 'FencedCodeWidget'),
    false,
    'no widget when language is not registered',
  )
  // The opener-line replace still fires (legacy behaviour).
  assert.ok(
    items.some((i) => i.replace && i.from === 0 && i.to === '```nonesuch-bl008'.length),
    'opener-line replace still emitted off-cursor for unregistered language',
  )
})

test('FencedCodeWidget.eq: source / language / generation triple gates equality', () => {
  const a = new FencedCodeWidget('graph TD\nA-->B', 'mermaid', 1)
  const b = new FencedCodeWidget('graph TD\nA-->B', 'mermaid', 1)
  const c = new FencedCodeWidget('graph TD\nA-->C', 'mermaid', 1)
  const d = new FencedCodeWidget('graph TD\nA-->B', 'plantuml', 1)
  const e = new FencedCodeWidget('graph TD\nA-->B', 'mermaid', 2)
  assert.equal(a.eq(b), true)
  assert.equal(a.eq(c), false, 'different source ⇒ not eq')
  assert.equal(a.eq(d), false, 'different language ⇒ not eq')
  assert.equal(a.eq(e), false, 'different generation ⇒ not eq')
})

// ── BL-125: viewport-scoped split between block + inline sources ────────────

import {
  buildLivePreviewBlockDecorations,
  buildLivePreviewInlineDecorations,
} from './livePreviewDecorations.ts'
import { ensureSyntaxTree } from '@codemirror/language'

interface Range {
  from: number
  to: number
}

function makeState(doc: string, selection?: EditorSelection) {
  const state = EditorState.create({
    doc,
    selection: selection ?? EditorSelection.cursor(0),
    extensions: [
      EditorState.allowMultipleSelections.of(true),
      markdown({ extensions: [Table] }),
    ],
  })
  // Force a complete lezer parse so viewport-bounded iteration sees
  // the full tree (the parser is incremental + time-bounded; without
  // this large docs are silently truncated).
  ensureSyntaxTree(state, doc.length, 30_000)
  return state
}

function decosFromSet(set: DecorationSet): Item[] {
  const items: Item[] = []
  const cur = set.iter()
  while (cur.value) {
    const spec = cur.value.spec as { class?: string; widget?: unknown; block?: boolean }
    const startSide = (cur.value as unknown as { startSide?: number }).startSide
    const isLine = startSide !== undefined && startSide < 0 && spec.class !== undefined
    const isReplace =
      cur.value.spec &&
      'inclusive' in (cur.value.spec as Record<string, unknown>) === false &&
      spec.class === undefined
    if (isLine) {
      items.push({ from: cur.from, to: cur.to, line: spec.class })
    } else if (isReplace && cur.from !== cur.to) {
      const widgetName = spec.widget
        ? (spec.widget as { constructor: { name: string } }).constructor.name
        : undefined
      items.push({
        from: cur.from,
        to: cur.to,
        replace: true,
        widget: widgetName,
        block: spec.block === true,
      })
    } else if (spec.class !== undefined) {
      items.push({ from: cur.from, to: cur.to, cls: spec.class })
    } else {
      const widgetName = spec.widget
        ? (spec.widget as { constructor: { name: string } }).constructor.name
        : undefined
      items.push({
        from: cur.from,
        to: cur.to,
        replace: true,
        widget: widgetName,
        block: spec.block === true,
      })
    }
    cur.next()
  }
  return items
}

test('BL-125 block source: emits HR / Table / FencedCode widgets, skips inline marks', () => {
  // Multi-construct doc — HR, table, paragraph with bold + inline code.
  // The block source should pick up HR + Table; the inline source
  // should pick up the bold mark + inline code mark.
  const doc = [
    '**bold** and `code`',
    '',
    '---',
    '',
    '| h | h2 |',
    '| - | -- |',
    '| 1 | 2  |',
  ].join('\n')
  // Cursor on line 1 (offset 0). HR and table need to be inactive
  // (i.e. cursor not on their lines) for the block-render path to
  // fire — putting the cursor at doc end would land it on the last
  // table row and suppress the widget.
  const state = makeState(doc, EditorSelection.cursor(0))
  const blockSet = buildLivePreviewBlockDecorations(state)
  const items = decosFromSet(blockSet)

  // HR widget over the `---` line (offsets 21..24).
  assert.ok(
    items.some(
      (i) =>
        i.replace &&
        i.widget === 'HrWidget' &&
        i.block === true &&
        i.from === 21 &&
        i.to === 24,
    ),
    'HR widget present in block source',
  )

  // Table widget covering the table lines.
  assert.ok(
    items.some(
      (i) => i.replace && i.widget === 'TableWidget' && i.block === true,
    ),
    'Table widget present in block source',
  )

  // No inline marks (those belong to the inline source).
  assert.equal(
    items.some((i) => i.cls === 'cm-md-strong'),
    false,
    'block source emits no inline strong mark',
  )
  assert.equal(
    items.some((i) => i.cls === 'cm-md-code'),
    false,
    'block source emits no inline code mark',
  )
})

test('BL-125 inline source: emits inline marks within the requested range only', () => {
  // Two emphasis runs on separate lines. Asking the inline builder
  // for just the first line should yield decorations only for that
  // line — the second line is "outside the viewport" for this call.
  const doc = ['**first**', '*second*'].join('\n')
  // doc layout:
  //   "**first**" → offsets 0..9
  //   "\n"        → offset 9
  //   "*second*"  → offsets 10..18
  const state = makeState(doc, EditorSelection.cursor(doc.length))
  const ranges: Range[] = [{ from: 0, to: 9 }]
  const items = decosFromSet(buildLivePreviewInlineDecorations(state, ranges))

  // First-line strong mark present.
  assert.ok(
    items.some((i) => i.cls === 'cm-md-strong'),
    'first-line strong mark emitted',
  )
  // Second-line emphasis mark absent — outside the requested range.
  assert.equal(
    items.some((i) => i.cls === 'cm-md-em'),
    false,
    'second-line emphasis mark NOT emitted — viewport excluded it',
  )
})

test('BL-125 inline source: empty range list emits nothing', () => {
  const doc = '**bold**\n*italic*'
  const state = makeState(doc, EditorSelection.cursor(doc.length))
  const items = decosFromSet(buildLivePreviewInlineDecorations(state, []))
  assert.equal(items.length, 0, 'empty ranges produces empty decoration set')
})

test('BL-125 inline source: heading line decoration appears within range', () => {
  const doc = ['# Hello', '', 'paragraph'].join('\n')
  const state = makeState(doc, EditorSelection.cursor(doc.length))
  // Heading line is offsets 0..7.
  const items = decosFromSet(
    buildLivePreviewInlineDecorations(state, [{ from: 0, to: 7 }]),
  )
  assert.ok(
    items.some((i) => i.line === 'cm-md-h1' && i.from === 0),
    'h1 line decoration emitted for visible heading',
  )
})

test('BL-125 inline source: selection-driven reveal depends on full doc state', () => {
  // The cursor on the heading line reveals the leading `# ` even when
  // the inline walk is range-bounded — the active-line computation
  // uses the global selection, not the supplied range. Pin that
  // behaviour so a viewport that starts past the cursor line doesn't
  // accidentally hide a "reveal" the user expects.
  const doc = '# Heading'
  const state = makeState(doc, EditorSelection.cursor(0))
  const items = decosFromSet(
    buildLivePreviewInlineDecorations(state, [{ from: 0, to: doc.length }]),
  )
  // No replace over the `# ` marker — cursor is on the heading.
  assert.equal(
    items.some((i) => i.replace && i.from === 0 && i.to <= 2),
    false,
    'cursor-on-heading: no replace over the # marker',
  )
  // Line decoration still applies.
  assert.ok(
    items.some((i) => i.line === 'cm-md-h1' && i.from === 0),
    'h1 line decoration still applied',
  )
})

test('BL-125: combined block + inline matches the full-walk reference', () => {
  // Sanity check that splitting the walk doesn't drop decorations
  // overall — for any doc, block-decos ∪ full-range-inline-decos
  // covers the same construct set as the legacy combined walker.
  const doc = [
    '# Heading',
    '',
    '**bold** then `code`',
    '',
    '---',
    '',
    'plain',
  ].join('\n')
  const state = makeState(doc, EditorSelection.cursor(doc.length))

  const fullItems = decosFromSet(buildLivePreviewDecorations(state))
  const blockItems = decosFromSet(buildLivePreviewBlockDecorations(state))
  const inlineItems = decosFromSet(
    buildLivePreviewInlineDecorations(state, [{ from: 0, to: doc.length }]),
  )

  // Sort both sides by (from, to, cls/line/widget) for comparison.
  const norm = (xs: Item[]) =>
    xs
      .slice()
      .sort(
        (a, b) =>
          a.from - b.from ||
          a.to - b.to ||
          (a.cls ?? a.line ?? a.widget ?? '').localeCompare(
            b.cls ?? b.line ?? b.widget ?? '',
          ),
      )
  const combined = norm([...blockItems, ...inlineItems])
  const reference = norm(fullItems)
  assert.deepEqual(combined, reference, 'split sources reproduce the full walk')
})

// ── C1 (#354) — whole-line image block widget ─────────────────────

import {
  forgeImageContext,
  ForgeImageWidget,
  type ForgeImageContext,
} from './livePreviewDecorations.ts'

const testImageContext: ForgeImageContext = {
  noteRelpath: 'notes/a.md',
  loadImage: async () => 'data:image/png;base64,SGk=',
}

function imageDecosFor(doc: string, ctx: ForgeImageContext | null, cursor = 0) {
  const state = EditorState.create({
    doc,
    selection: EditorSelection.cursor(cursor),
    extensions: [
      markdown({ extensions: [Table] }),
      ...(ctx ? [forgeImageContext.of(ctx)] : []),
    ],
  })
  const set: DecorationSet = buildLivePreviewDecorations(state)
  const out: Array<{ from: number; to: number; widget: unknown; block: boolean }> = []
  const cur = set.iter()
  while (cur.value) {
    const spec = cur.value.spec as { widget?: unknown; block?: boolean }
    if (spec.widget instanceof ForgeImageWidget) {
      out.push({
        from: cur.from,
        to: cur.to,
        widget: spec.widget,
        block: spec.block === true,
      })
    }
    cur.next()
  }
  return out
}

test('whole-line image swaps to a ForgeImageWidget block replace', () => {
  const doc = '![alt](img.png)\n\ntrailing text'
  const widgets = imageDecosFor(doc, testImageContext, doc.length)
  assert.equal(widgets.length, 1)
  assert.equal(widgets[0]!.block, true)
  assert.equal(widgets[0]!.from, 0)
  assert.equal(widgets[0]!.to, '![alt](img.png)'.length)
  const w = widgets[0]!.widget as ForgeImageWidget
  assert.equal(w.src, 'img.png')
  assert.equal(w.alt, 'alt')
})

test('image on the active line keeps its syntax (no widget)', () => {
  const doc = '![alt](img.png)\n\ntrailing text'
  const widgets = imageDecosFor(doc, testImageContext, 3)
  assert.equal(widgets.length, 0)
})

test('inline image mixed with text keeps mark-only styling', () => {
  const doc = 'see ![alt](img.png) here\n\nend'
  const widgets = imageDecosFor(doc, testImageContext, doc.length)
  assert.equal(widgets.length, 0)
})

test('no forgeImageContext → no widget (v1 behaviour preserved)', () => {
  const doc = '![alt](img.png)\n\ntrailing text'
  const widgets = imageDecosFor(doc, null, doc.length)
  assert.equal(widgets.length, 0)
})
