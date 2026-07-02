import { syntaxTree } from '@codemirror/language'
import { Facet, type EditorState } from '@codemirror/state'
import { Decoration, type DecorationSet, WidgetType } from '@codemirror/view'
import { renderMarkdown } from '../markdownRender'
import { fencedCodeRegistry } from './fencedCodeRegistry'

// ── C1 (#354) — forge-image rendering context ────────────────────────
//
// The image block widget needs to turn a markdown src into displayable
// bytes, which requires the note's relpath (relative resolution) and a
// kernel handle (storage read). Both arrive via this facet, provided
// by `livePreviewExt({ forgeImages })`; when absent (tests, tabs
// without a kernel), images keep the v1 mark-only styling.

export interface ForgeImageContext {
  /** Relpath of the note being edited — part of the widget identity so
   *  switching tabs re-resolves relative srcs. */
  noteRelpath: string
  /** Resolve a (possibly relative, URI-encoded) image src to a
   *  data: URL, or `null` when the file isn't in the forge. */
  loadImage: (src: string) => Promise<string | null>
}

export const forgeImageContext = Facet.define<
  ForgeImageContext,
  ForgeImageContext | null
>({
  combine: (values) => values[0] ?? null,
})

export class ForgeImageWidget extends WidgetType {
  constructor(
    readonly src: string,
    readonly alt: string,
    readonly context: ForgeImageContext,
  ) {
    super()
  }
  eq(other: ForgeImageWidget): boolean {
    return (
      this.src === other.src &&
      this.alt === other.alt &&
      this.context.noteRelpath === other.context.noteRelpath
    )
  }
  toDOM(): HTMLElement {
    const wrap = document.createElement('div')
    wrap.className = 'cm-md-image-widget'
    wrap.dataset.src = this.src
    const img = document.createElement('img')
    img.alt = this.alt
    img.className = 'nx-forge-image'
    wrap.appendChild(img)
    void this.context.loadImage(this.src).then((url) => {
      if (!wrap.isConnected) return
      if (url) {
        img.src = url
      } else {
        wrap.replaceChildren()
        const chip = document.createElement('span')
        chip.className = 'cm-md-image-missing'
        chip.textContent = `image not found: ${this.src}`
        wrap.appendChild(chip)
      }
    })
    return wrap
  }
  ignoreEvent(): boolean {
    return true
  }
}

class HrWidget extends WidgetType {
  eq(_other: HrWidget): boolean {
    return true
  }
  toDOM(): HTMLElement {
    const hr = document.createElement('hr')
    hr.className = 'cm-md-hr-widget'
    return hr
  }
  ignoreEvent(): boolean {
    return true
  }
}

export class TableWidget extends WidgetType {
  constructor(readonly source: string) {
    super()
  }
  eq(other: TableWidget): boolean {
    return this.source === other.source
  }
  toDOM(): HTMLElement {
    const wrap = document.createElement('div')
    wrap.className = 'cm-md-table-widget nexus-markdown-body'
    wrap.innerHTML = renderMarkdown(this.source)
    return wrap
  }
  ignoreEvent(): boolean {
    return true
  }
}

export class FencedCodeWidget extends WidgetType {
  constructor(
    readonly source: string,
    readonly language: string,
    readonly generation: number,
  ) {
    super()
  }
  eq(other: FencedCodeWidget): boolean {
    return (
      this.language === other.language &&
      this.generation === other.generation &&
      this.source === other.source
    )
  }
  toDOM(): HTMLElement {
    const wrap = document.createElement('div')
    wrap.className = 'cm-md-fenced-widget'
    wrap.dataset.language = this.language
    const sync = fencedCodeRegistry.renderCached(this.language, this.source)
    if (sync) {
      wrap.appendChild(sync)
      return wrap
    }
    const placeholder = document.createElement('pre')
    placeholder.className = 'cm-md-fenced-pending'
    const code = document.createElement('code')
    code.textContent = this.source
    placeholder.appendChild(code)
    wrap.appendChild(placeholder)
    const pending = fencedCodeRegistry.awaitPending(this.language, this.source)
    if (pending) {
      void pending.then((result) => {
        if (!wrap.isConnected) return
        wrap.replaceChildren()
        if (result instanceof Error) {
          wrap.appendChild(buildFencedErrorElement(this.language, result))
        } else {
          wrap.appendChild(result)
        }
      })
    }
    return wrap
  }
  ignoreEvent(): boolean {
    return true
  }
}

function buildFencedErrorElement(language: string, err: Error): HTMLElement {
  const box = document.createElement('div')
  box.className = 'cm-md-fenced-error'
  const tag = document.createElement('span')
  tag.className = 'cm-md-fenced-error-lang'
  tag.textContent = language
  const msg = document.createElement('span')
  msg.className = 'cm-md-fenced-error-msg'
  msg.textContent = err.message || 'render failed'
  box.append(tag, msg)
  return box
}

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
 *
 * BL-125: see also [`buildLivePreviewBlockDecorations`] and
 * [`buildLivePreviewInlineDecorations`] for the viewport-scoped
 * split — production code wires those through a `StateField` (blocks)
 * + `ViewPlugin` (inline) pair so the inline walk cost is bounded by
 * viewport size, not document size. This combined entry-point stays
 * for backward compat (the existing unit tests + the BL-122 perf
 * harness drive it directly).
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

/**
 * BL-125 — emit only block-level decorations (HR widget, table widget,
 * fenced-code widget) by walking the full syntax tree.
 *
 * CM6's atomic-range + block-decoration semantics require block
 * widgets to come from a `StateField` (not a `ViewPlugin`), so this
 * builder is the one wired into the field side of the split. The
 * full-tree walk is OK because block constructs are rare (a typical
 * doc has < 50 tables / HRs / fenced blocks) and each handler
 * descends only into the matched node's children.
 *
 * Inline decorations are emitted by [`buildLivePreviewInlineDecorations`].
 */
export function buildLivePreviewBlockDecorations(state: EditorState): DecorationSet {
  const activeLines = computeActiveLines(state)
  const items: DecorationItem[] = []
  const tree = syntaxTree(state)
  const doc = state.doc

  tree.iterate({
    enter(node) {
      visitBlock(node, doc, state, activeLines, items)
    },
  })

  return decorationsFromItems(items)
}

/**
 * BL-125 — emit inline marks, line decorations, and non-block replace
 * ranges by walking only the supplied `ranges` (typically
 * `view.visibleRanges`).
 *
 * Marks / replaces / line decorations outside the visible viewport
 * have no effect on the rendered output, and the user can't see them.
 * So the walker is bounded by viewport size, not document size — the
 * core BL-125 win.
 *
 * Atomic-ranges integration: the inline `Decoration.replace` ranges
 * emitted here are wired into `EditorView.atomicRanges` alongside the
 * block field's. Atomic ranges only matter for cursor motion inside
 * visible content; navigation that jumps past the viewport (e.g.
 * Ctrl+End) lands the cursor inside the freshly-recomputed viewport's
 * decoration set, since CM6 fires a `viewportChanged` update before
 * the next paint.
 */
export function buildLivePreviewInlineDecorations(
  state: EditorState,
  ranges: readonly { from: number; to: number }[],
): DecorationSet {
  const activeLines = computeActiveLines(state)
  const items: DecorationItem[] = []
  const tree = syntaxTree(state)
  const doc = state.doc

  for (const r of ranges) {
    if (r.to <= r.from) continue
    tree.iterate({
      enter(node) {
        visitInline(node, doc, state, activeLines, items)
      },
      from: r.from,
      to: r.to,
    })
  }

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
    // C1 (#354) — whole-line images swap to a block widget (emitted
    // here so the combined builder's output matches the field+plugin
    // union); everything else keeps the v1 mark-only styling.
    if (!handleImageBlock(node.node, doc, state, active, items)) {
      handleImage(node.node, items)
    }
    return
  }
  if (/^ATXHeading[1-6]$/.test(name)) {
    handleAtxHeading(node.node, reveal, doc, items)
    return
  }
  if (name === 'SetextHeading1' || name === 'SetextHeading2') {
    handleSetextHeading(node.node, reveal, doc, items)
    handleSetextHeadingBlock(node.node, state, active, doc, items)
    return
  }
  if (name === 'HorizontalRule') {
    handleHorizontalRule(node.node, reveal, doc, items)
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
  if (name === 'FencedCode') {
    handleFencedCode(node.node, doc, active, state, items)
    return
  }
  if (name === 'CodeBlock') {
    handleCodeBlock(node.node, doc, items)
    return
  }
  if (name === 'HTMLBlock' || name === 'HTMLTag') {
    pushMark(items, node.from, node.to, 'cm-md-html')
    return
  }
  if (name === 'Table') {
    handleTable(node.node, doc, active, state, items)
    return
  }
}

/**
 * BL-125 — block-only visitor for the StateField source. Emits
 * `Decoration.replace({ block: true, widget })` for HR / Table /
 * FencedCode (when the block-render path fires) and nothing else.
 * Skipped block constructs (HR on active line, Table on active line,
 * FencedCode without a registered renderer or on the active line)
 * yield zero output here — the inline visitor handles their fallback
 * line decorations.
 */
function visitBlock(
  node: SyntaxNodeRef,
  doc: EditorState['doc'],
  state: EditorState,
  active: Set<number>,
  items: DecorationItem[],
): void {
  const name = node.name
  if (name === 'HorizontalRule') {
    const reveal = nodeIntersectsActiveLines(state, node.from, node.to, active)
    handleHorizontalRule(node.node, reveal, doc, items)
    return
  }
  if (name === 'Table') {
    handleTable(node.node, doc, active, state, items)
    return
  }
  if (name === 'FencedCode') {
    handleFencedCodeBlockOnly(node.node, doc, active, state, items)
    return
  }
  if (name === 'SetextHeading1' || name === 'SetextHeading2') {
    handleSetextHeadingBlock(node.node, state, active, doc, items)
    return
  }
  if (name === 'Image') {
    // C1 (#354) — block-widget swap for whole-line images. CM6
    // requires block widgets to originate from the StateField source,
    // same constraint as tables / HRs.
    handleImageBlock(node.node, doc, state, active, items)
    return
  }
}

/**
 * C1 (#354) — swap a whole-line `![alt](src)` for a rendered image
 * block widget. Fires only when:
 *   - a [`forgeImageContext`] is installed (kernel-backed tab),
 *   - the image is the *entire* trimmed content of a single line
 *     (mid-sentence images keep mark-only styling — a block swap
 *     would reflow the surrounding text), and
 *   - the line isn't active (cursor moves onto it → syntax reveals,
 *     mirroring the table behaviour).
 * Returns `true` when the widget was emitted so inline callers can
 * skip the mark fallback for the same node.
 */
function handleImageBlock(
  node: SyntaxNode,
  doc: EditorState['doc'],
  state: EditorState,
  active: Set<number>,
  items: DecorationItem[],
): boolean {
  const ctx = state.facet(forgeImageContext)
  if (!ctx) return false
  const startLine = doc.lineAt(node.from)
  if (doc.lineAt(node.to).number !== startLine.number) return false
  if (active.has(startLine.number)) return false
  if (startLine.text.trim() !== doc.sliceString(node.from, node.to).trim()) {
    return false
  }
  let src = ''
  const marks: { from: number; to: number }[] = []
  let cur: SyntaxNode | null = node.firstChild
  while (cur) {
    if (cur.name === 'URL') src = doc.sliceString(cur.from, cur.to)
    else if (cur.name === 'LinkMark') marks.push({ from: cur.from, to: cur.to })
    cur = cur.nextSibling
  }
  // Angle-bracketed destinations (`![](<a b.png>)`) include the
  // brackets in the URL node — strip them.
  src = src.replace(/^<|>$/g, '').trim()
  if (src.length === 0) return false
  const alt =
    marks.length >= 2 && marks[1]!.from > marks[0]!.to
      ? doc.sliceString(marks[0]!.to, marks[1]!.from)
      : ''
  items.push({
    from: startLine.from,
    to: startLine.to,
    deco: Decoration.replace({
      widget: new ForgeImageWidget(src, alt, ctx),
      block: true,
      inclusive: false,
    }),
  })
  return true
}

/** Predicate twin of [`handleImageBlock`] for the inline visitor:
 *  runs the exact same logic against a throwaway items array so the
 *  two sources can never drift. Widget construction without `toDOM`
 *  is allocation-only, so the cost is negligible. */
function imageBlockOwned(
  node: SyntaxNode,
  doc: EditorState['doc'],
  state: EditorState,
  active: Set<number>,
): boolean {
  return handleImageBlock(node, doc, state, active, [])
}

// The cross-line replace that hides the `===` / `---` underline row
// crosses a `\n` and therefore must live in the StateField source
// (CM6: "Decorations that replace line breaks may not be specified
// via plugins"). The matching inline path emits only the
// per-line `cm-md-h{n}` line decorations and never the replace.
function handleSetextHeadingBlock(
  node: SyntaxNode,
  state: EditorState,
  active: Set<number>,
  doc: EditorState['doc'],
  items: DecorationItem[],
): void {
  const reveal = nodeIntersectsActiveLines(state, node.from, node.to, active)
  if (reveal) return
  const startLine = doc.lineAt(node.from)
  const endLine = doc.lineAt(node.to)
  if (endLine.number <= startLine.number) return
  const underline = endLine
  const hideFrom = underline.from > 0 ? underline.from - 1 : underline.from
  items.push({
    from: hideFrom,
    to: underline.to,
    deco: Decoration.replace({ block: true, inclusive: false }),
  })
}

/**
 * BL-125 — inline visitor for the ViewPlugin source. Mirrors
 * [`visit`] but skips constructs whose only output is a block widget
 * (HR, Table). For FencedCode it emits only the line-decoration /
 * inline-replace fallback path; the block widget itself is owned by
 * the StateField via [`visitBlock`].
 */
function visitInline(
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
    // C1 (#354) — the StateField source owns whole-line images (block
    // widget); emit the mark fallback only when it doesn't fire.
    if (!imageBlockOwned(node.node, doc, state, active)) {
      handleImage(node.node, items)
    }
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
  if (name === 'FencedCode') {
    handleFencedCodeInlineOnly(node.node, doc, active, state, items)
    return
  }
  if (name === 'CodeBlock') {
    handleCodeBlock(node.node, doc, items)
    return
  }
  if (name === 'HTMLBlock' || name === 'HTMLTag') {
    pushMark(items, node.from, node.to, 'cm-md-html')
    return
  }
}

function handleTable(
  node: SyntaxNode,
  doc: EditorState['doc'],
  active: Set<number>,
  state: EditorState,
  items: DecorationItem[],
): void {
  const startLine = doc.lineAt(node.from)
  const endLine = doc.lineAt(node.to)
  for (let l = startLine.number; l <= endLine.number; l++) {
    if (active.has(l)) return
  }
  const source = state.doc.sliceString(startLine.from, endLine.to)
  items.push({
    from: startLine.from,
    to: endLine.to,
    deco: Decoration.replace({
      widget: new TableWidget(source),
      block: true,
      inclusive: false,
    }),
  })
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
  if (reveal || endLine.number === startLine.number) {
    // Apply the heading line decoration to the underline row when
    // revealed (or absent — single-line edge case) so the visible row
    // keeps the heading scale. The off-cursor underline-row hide
    // happens in `handleSetextHeadingBlock` (StateField source) since
    // its replace range crosses a newline.
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

function handleFencedCode(
  node: SyntaxNode,
  doc: EditorState['doc'],
  active: Set<number>,
  state: EditorState,
  items: DecorationItem[],
): void {
  // Backward-compat path for the combined `buildLivePreviewDecorations`
  // entry-point. The split visitors below run only their relevant
  // branch.
  if (fencedCodeIsBlockRendering(node, doc, active, state)) {
    handleFencedCodeBlockOnly(node, doc, active, state, items)
    return
  }
  handleFencedCodeInlineOnly(node, doc, active, state, items)
}

function fencedCodeIsBlockRendering(
  node: SyntaxNode,
  doc: EditorState['doc'],
  active: Set<number>,
  state: EditorState,
): boolean {
  const startLine = doc.lineAt(node.from)
  const endLine = doc.lineAt(node.to)
  const language = readFencedCodeLanguage(node, state)
  return (
    !!language &&
    fencedCodeRegistry.has(language) &&
    !nodeIntersectsActiveLines(state, startLine.from, endLine.to, active)
  )
}

function handleFencedCodeBlockOnly(
  node: SyntaxNode,
  doc: EditorState['doc'],
  active: Set<number>,
  state: EditorState,
  items: DecorationItem[],
): void {
  const startLine = doc.lineAt(node.from)
  const endLine = doc.lineAt(node.to)
  const language = readFencedCodeLanguage(node, state)
  if (
    !language ||
    !fencedCodeRegistry.has(language) ||
    nodeIntersectsActiveLines(state, startLine.from, endLine.to, active)
  ) {
    return
  }
  const innerSource = readFencedCodeBody(node, state, startLine, endLine)
  items.push({
    from: startLine.from,
    to: endLine.to,
    deco: Decoration.replace({
      widget: new FencedCodeWidget(
        innerSource,
        language,
        fencedCodeRegistry.generation(),
      ),
      block: true,
      inclusive: false,
    }),
  })
}

function handleFencedCodeInlineOnly(
  node: SyntaxNode,
  doc: EditorState['doc'],
  active: Set<number>,
  state: EditorState,
  items: DecorationItem[],
): void {
  // When the block-render path fires this fence is owned by the block
  // StateField; the inline plugin emits nothing.
  if (fencedCodeIsBlockRendering(node, doc, active, state)) return

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
  const fenceLines = new Set<number>([startLine.number])
  if (endLine.number !== startLine.number) fenceLines.add(endLine.number)
  for (const lineNo of fenceLines) {
    if (nodeIntersectsActiveLines(state, doc.line(lineNo).from, doc.line(lineNo).to, active)) continue
    const line = doc.line(lineNo)
    if (line.to > line.from) pushReplace(items, line.from, line.to)
  }
}

function readFencedCodeLanguage(node: SyntaxNode, state: EditorState): string | null {
  let cur: SyntaxNode | null = node.firstChild
  while (cur) {
    if (cur.name === 'CodeInfo') {
      return state.doc.sliceString(cur.from, cur.to).trim().split(/\s+/)[0] ?? null
    }
    cur = cur.nextSibling
  }
  return null
}

function readFencedCodeBody(
  node: SyntaxNode,
  state: EditorState,
  startLine: ReturnType<EditorState['doc']['lineAt']>,
  endLine: ReturnType<EditorState['doc']['lineAt']>,
): string {
  let cur: SyntaxNode | null = node.firstChild
  let bodyFrom = -1
  let bodyTo = -1
  while (cur) {
    if (cur.name === 'CodeText') {
      if (bodyFrom < 0) bodyFrom = cur.from
      bodyTo = cur.to
    }
    cur = cur.nextSibling
  }
  if (bodyFrom < 0 || bodyTo < bodyFrom) {
    const innerStart = startLine.number < endLine.number ? startLine.to + 1 : startLine.to
    const innerEnd = endLine.number > startLine.number ? endLine.from - 1 : endLine.to
    if (innerEnd <= innerStart) return ''
    return state.doc.sliceString(innerStart, innerEnd)
  }
  return state.doc.sliceString(bodyFrom, bodyTo)
}

function handleHorizontalRule(
  node: SyntaxNode,
  reveal: boolean,
  doc: EditorState['doc'],
  items: DecorationItem[],
): void {
  if (reveal) return
  const line = doc.lineAt(node.from)
  items.push({
    from: line.from,
    to: line.to,
    deco: Decoration.replace({ widget: new HrWidget(), block: true, inclusive: false }),
  })
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
