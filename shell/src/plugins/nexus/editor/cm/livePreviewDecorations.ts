import { syntaxTree } from '@codemirror/language'
import type { EditorState } from '@codemirror/state'
import { Decoration, type DecorationSet } from '@codemirror/view'

// Minimal shape of the @lezer/common nodes we touch — re-declared
// locally so we don't take a direct dependency on the transitive
// package. Matches the public API surface verified at runtime.
interface SyntaxNode {
  name: string
  from: number
  to: number
  firstChild: SyntaxNode | null
  nextSibling: SyntaxNode | null
}
interface SyntaxNodeRef {
  name: string
  from: number
  to: number
  node: SyntaxNode
}

/**
 * Live-preview decoration builder.
 *
 * Walks the markdown syntax tree and emits two flavours of decoration:
 *
 *   - **Marks** (`Decoration.mark`) for inline styling — italic, bold,
 *     code, link colour, etc. Always emitted regardless of cursor
 *     position, since the user expects styled text whether or not their
 *     cursor is on the line.
 *   - **Replaces** (`Decoration.replace`) that hide markdown syntax
 *     marks (the `*`, `**`, `[]()`, `#`s, ...) when the cursor is
 *     *not* on the same line. Lines with the active selection
 *     (collapsed cursor or a non-empty range that touches the line)
 *     have their marks revealed so the user can edit them.
 *
 * Active-line set: every line touched by `state.selection.ranges` (head
 * or anchor, inclusive of the full selection span). Multi-cursor + range
 * selections all contribute lines.
 *
 * Atomicity: the caller pairs the resulting set with
 * `EditorView.atomicRanges` so cursor motion skips over hidden marks
 * cleanly — a left-arrow doesn't park inside an invisible `**`.
 */
export function buildLivePreviewDecorations(state: EditorState): DecorationSet {
  const activeLines = computeActiveLines(state)
  const items: DecorationItem[] = []
  const tree = syntaxTree(state)
  const doc = state.doc

  tree.iterate({
    enter(node) {
      visit(node, doc, state, activeLines, items)
    },
  })

  return decorationsFromItems(items)
}

interface DecorationItem {
  from: number
  to: number
  deco: Decoration
}

function computeActiveLines(state: EditorState): Set<number> {
  const lines = new Set<number>()
  for (const range of state.selection.ranges) {
    const fromLine = state.doc.lineAt(range.from).number
    const toLine = state.doc.lineAt(range.to).number
    for (let i = fromLine; i <= toLine; i++) lines.add(i)
    lines.add(state.doc.lineAt(range.anchor).number)
    lines.add(state.doc.lineAt(range.head).number)
  }
  return lines
}

function nodeIntersectsActiveLines(
  state: EditorState,
  from: number,
  to: number,
  active: Set<number>,
): boolean {
  if (active.size === 0) return false
  const fromLine = state.doc.lineAt(from).number
  const toLine = state.doc.lineAt(to).number
  for (let i = fromLine; i <= toLine; i++) {
    if (active.has(i)) return true
  }
  return false
}

const HIDE_MARK = Decoration.replace({})

function pushReplace(items: DecorationItem[], from: number, to: number): void {
  if (from >= to) return
  items.push({ from, to, deco: HIDE_MARK })
}

function pushMark(items: DecorationItem[], from: number, to: number, cls: string): void {
  if (from >= to) return
  items.push({ from, to, deco: Decoration.mark({ class: cls }) })
}

function pushLine(items: DecorationItem[], from: number, cls: string): void {
  items.push({ from, to: from, deco: Decoration.line({ class: cls }) })
}

function visit(
  node: SyntaxNodeRef,
  doc: EditorState['doc'],
  state: EditorState,
  active: Set<number>,
  items: DecorationItem[],
): void {
  const name = node.name
  const reveal = nodeIntersectsActiveLines(state, node.from, node.to, active)

  if (name === 'Emphasis') {
    handleEmphasis(node.node, reveal, items, 'cm-md-em')
    return
  }
  if (name === 'StrongEmphasis') {
    handleEmphasis(node.node, reveal, items, 'cm-md-strong')
    return
  }
  if (name === 'InlineCode') {
    handleInlineCode(node.node, reveal, items)
    return
  }
  if (name === 'Link') {
    handleLink(node.node, reveal, items)
    return
  }
  if (name === 'Image') {
    handleImage(node.node, items)
    return
  }
  if (/^ATXHeading[1-6]$/.test(name)) {
    handleAtxHeading(node.node, reveal, doc, items)
    return
  }
  if (name === 'SetextHeading1' || name === 'SetextHeading2') {
    handleSetextHeading(node.node, reveal, doc, items)
    return
  }
  if (name === 'HorizontalRule') {
    const line = doc.lineAt(node.from)
    pushLine(items, line.from, 'cm-md-hr-line')
    return
  }
  if (name === 'Blockquote') {
    handleBlockquote(node.node, doc, items)
    return
  }
  if (name === 'BulletList' || name === 'OrderedList') {
    return
  }
  if (name === 'ListMark') {
    pushMark(items, node.from, node.to, 'cm-md-list-marker')
    return
  }
  if (name === 'TaskMarker') {
    pushMark(items, node.from, node.to, 'cm-md-task')
    return
  }
  if (name === 'FencedCode' || name === 'CodeBlock') {
    handleCodeBlock(node.node, doc, items)
    return
  }
  if (name === 'HTMLBlock' || name === 'HTMLTag') {
    pushMark(items, node.from, node.to, 'cm-md-html')
    return
  }
}

function handleEmphasis(
  node: SyntaxNode,
  reveal: boolean,
  items: DecorationItem[],
  cls: string,
): void {
  const marks: { from: number; to: number }[] = []
  let inner: { from: number; to: number } | null = null
  let cur: SyntaxNode | null = node.firstChild
  while (cur) {
    if (cur.name === 'EmphasisMark') {
      marks.push({ from: cur.from, to: cur.to })
    }
    cur = cur.nextSibling
  }
  if (marks.length >= 2) {
    inner = { from: marks[0]!.to, to: marks[marks.length - 1]!.from }
  } else {
    inner = { from: node.from, to: node.to }
  }
  if (!reveal) {
    for (const m of marks) pushReplace(items, m.from, m.to)
  }
  if (inner.to > inner.from) {
    pushMark(items, inner.from, inner.to, cls)
  }
}

function handleInlineCode(
  node: SyntaxNode,
  reveal: boolean,
  items: DecorationItem[],
): void {
  const marks: { from: number; to: number }[] = []
  let cur: SyntaxNode | null = node.firstChild
  while (cur) {
    if (cur.name === 'CodeMark') {
      marks.push({ from: cur.from, to: cur.to })
    }
    cur = cur.nextSibling
  }
  let inner: { from: number; to: number }
  if (marks.length >= 2) {
    inner = { from: marks[0]!.to, to: marks[marks.length - 1]!.from }
  } else {
    inner = { from: node.from, to: node.to }
  }
  if (!reveal) {
    for (const m of marks) pushReplace(items, m.from, m.to)
  }
  if (inner.to > inner.from) {
    pushMark(items, inner.from, inner.to, 'cm-md-code')
  }
}

function handleLink(
  node: SyntaxNode,
  reveal: boolean,
  items: DecorationItem[],
): void {
  // Lezer markdown emits a Link with children in source order:
  //   LinkMark `[`, … inline content …, LinkMark `]`,
  //   LinkMark `(`, URL, (LinkTitle?), LinkMark `)`.
  // We collect every LinkMark and the URL/Title spans, then derive:
  //   - `cm-md-link` mark over the visible text (between marks[0].to
  //     and marks[1].from).
  //   - When off-cursor, two replace ranges: the leading `[` (marks[0])
  //     and the trailing `](url)` span (marks[1].from … node.to or
  //     marks[last].to — whichever is last).
  const marks: { from: number; to: number }[] = []
  let urlEnd = -1
  let cur: SyntaxNode | null = node.firstChild
  while (cur) {
    if (cur.name === 'LinkMark') marks.push({ from: cur.from, to: cur.to })
    else if (cur.name === 'URL' || cur.name === 'LinkTitle') {
      if (cur.to > urlEnd) urlEnd = cur.to
    }
    cur = cur.nextSibling
  }
  if (marks.length === 0) {
    pushMark(items, node.from, node.to, 'cm-md-link')
    return
  }
  const inner = { from: marks[0]!.to, to: marks[1]?.from ?? node.to }
  if (inner.to > inner.from) pushMark(items, inner.from, inner.to, 'cm-md-link')

  if (!reveal) {
    pushReplace(items, marks[0]!.from, marks[0]!.to)
    if (marks.length >= 2) {
      const lastMark = marks[marks.length - 1]!
      const trailEnd = Math.max(lastMark.to, urlEnd, node.to)
      pushReplace(items, marks[1]!.from, trailEnd)
    }
  }
}

function handleImage(node: SyntaxNode, items: DecorationItem[]): void {
  // v1: mark-only. The block-widget swap lands in Phase 2.
  const marks: { from: number; to: number }[] = []
  let cur: SyntaxNode | null = node.firstChild
  while (cur) {
    if (cur.name === 'LinkMark') marks.push({ from: cur.from, to: cur.to })
    cur = cur.nextSibling
  }
  if (marks.length >= 2) {
    const inner = { from: marks[0]!.to, to: marks[1]!.from }
    if (inner.to > inner.from) pushMark(items, inner.from, inner.to, 'cm-md-image')
  } else {
    pushMark(items, node.from, node.to, 'cm-md-image')
  }
}

function handleAtxHeading(
  node: SyntaxNode,
  reveal: boolean,
  doc: EditorState['doc'],
  items: DecorationItem[],
): void {
  const m = /^ATXHeading([1-6])$/.exec(node.name)
  if (!m) return
  const level = m[1]!
  const startLine = doc.lineAt(node.from)
  const endLine = doc.lineAt(node.to)
  for (let l = startLine.number; l <= endLine.number; l++) {
    pushLine(items, doc.line(l).from, `cm-md-h${level}`)
  }
  if (!reveal) {
    const cur: SyntaxNode | null = node.firstChild
    if (cur && cur.name === 'HeaderMark') {
      // Hide the leading `#`s plus any whitespace right after them.
      let hideTo = cur.to
      const lineText = startLine.text
      const offsetInLine = cur.to - startLine.from
      let i = offsetInLine
      while (i < lineText.length && (lineText[i] === ' ' || lineText[i] === '\t')) {
        i++
        hideTo++
      }
      pushReplace(items, cur.from, hideTo)
    }
  }
}

function handleSetextHeading(
  node: SyntaxNode,
  reveal: boolean,
  doc: EditorState['doc'],
  items: DecorationItem[],
): void {
  const level = node.name === 'SetextHeading1' ? '1' : '2'
  const startLine = doc.lineAt(node.from)
  const endLine = doc.lineAt(node.to)
  // Title line(s) get the heading line decoration. The underline is
  // the last line of the node.
  for (let l = startLine.number; l < endLine.number; l++) {
    pushLine(items, doc.line(l).from, `cm-md-h${level}`)
  }
  if (!reveal && endLine.number > startLine.number) {
    // Hide the underline line entirely (its content + its trailing
    // newline, if any, so the row collapses).
    const underline = endLine
    const hideFrom = underline.from > 0 ? underline.from - 1 : underline.from
    pushReplace(items, hideFrom, underline.to)
  } else {
    // Still apply heading line decoration to the underline row when
    // revealed so the visible row keeps the heading scale.
    pushLine(items, endLine.from, `cm-md-h${level}`)
  }
}

function handleBlockquote(
  node: SyntaxNode,
  doc: EditorState['doc'],
  items: DecorationItem[],
): void {
  const startLine = doc.lineAt(node.from)
  const endLine = doc.lineAt(node.to)
  for (let l = startLine.number; l <= endLine.number; l++) {
    pushLine(items, doc.line(l).from, 'cm-md-blockquote')
  }
  // Mark every direct QuoteMark child faded.
  let cur: SyntaxNode | null = node.firstChild
  while (cur) {
    if (cur.name === 'QuoteMark') {
      pushMark(items, cur.from, cur.to, 'cm-md-blockquote-mark')
    }
    cur = cur.nextSibling
  }
}

function handleCodeBlock(
  node: SyntaxNode,
  doc: EditorState['doc'],
  items: DecorationItem[],
): void {
  const startLine = doc.lineAt(node.from)
  const endLine = doc.lineAt(node.to)
  for (let l = startLine.number; l <= endLine.number; l++) {
    pushLine(items, doc.line(l).from, 'cm-md-codeblock')
  }
  let cur: SyntaxNode | null = node.firstChild
  while (cur) {
    if (cur.name === 'CodeMark') {
      pushMark(items, cur.from, cur.to, 'cm-md-fence')
    }
    cur = cur.nextSibling
  }
}

/**
 * `RangeSetBuilder` requires sorted, non-overlapping ranges plus a
 * stable `startSide` ordering. We collect first, sort, then build —
 * cheaper than carrying a Compactor through the recursive walk and
 * easier to keep correct as constructs are added.
 *
 * Sort key:
 *   1. `from` ascending.
 *   2. `to` descending — outer nodes (wider spans) come first so a
 *      block decoration over [0, 100] lands before a mark over [10, 20].
 *   3. Line decorations get the smallest start side via Decoration.line
 *      itself; we just preserve insertion order for ties.
 */
function decorationsFromItems(items: DecorationItem[]): DecorationSet {
  if (items.length === 0) return Decoration.none
  const sorted = items.slice().sort((a, b) => {
    if (a.from !== b.from) return a.from - b.from
    if (a.deco.spec?.block && !b.deco.spec?.block) return -1
    if (b.deco.spec?.block && !a.deco.spec?.block) return 1
    return b.to - a.to
  })
  // Use the rangeset builder by feeding ranges in sorted order via
  // Decoration.set, which accepts an unsorted-but-becomes-sorted input
  // and tolerates equal-position ranges.
  return Decoration.set(
    sorted.map(({ from, to, deco }) => deco.range(from, to)),
    true,
  )
}
