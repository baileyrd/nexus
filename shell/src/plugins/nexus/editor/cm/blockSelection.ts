// Phase 2 of docs/roadmap/notion-block-ux-plan.md — block selection.
//
// Cmd/Ctrl+A with a caret inside a block (a maximal run of non-blank
// lines) selects that block's text. A second Cmd/Ctrl+A while the
// current selection already matches a block's range expands to the
// whole document. Matches Notion's two-step behaviour.
//
// "Block" here is computed from the markdown text — runs of
// consecutive non-blank lines — rather than the kernel block tree.
// That's deliberately lighter than pulling the session snapshot on
// every keystroke and lines up with how the slash menu already
// treats blocks (Phase 1).

import { EditorSelection, type Extension } from '@codemirror/state'
import { keymap, type EditorView } from '@codemirror/view'

/** Given a document offset, return the inclusive line-range of the
 *  block that contains it. A block is a maximal run of consecutive
 *  non-blank lines. Blank lines belong to no block — Cmd+A on a
 *  blank line expands directly to the whole document. */
function blockRangeAt(view: EditorView, pos: number): { from: number; to: number } | null {
  const doc = view.state.doc
  const line = doc.lineAt(pos)
  if (line.text.trim() === '') return null
  let topLineNo = line.number
  while (topLineNo > 1 && doc.line(topLineNo - 1).text.trim() !== '') topLineNo--
  let bottomLineNo = line.number
  while (bottomLineNo < doc.lines && doc.line(bottomLineNo + 1).text.trim() !== '') bottomLineNo++
  return {
    from: doc.line(topLineNo).from,
    to: doc.line(bottomLineNo).to,
  }
}

/** True when the main selection already covers `range` exactly. */
function selectionMatches(view: EditorView, range: { from: number; to: number }): boolean {
  const sel = view.state.selection.main
  const lo = Math.min(sel.anchor, sel.head)
  const hi = Math.max(sel.anchor, sel.head)
  return lo === range.from && hi === range.to
}

/** `Mod-a` — expand selection from caret to containing block, then
 *  to whole document on a second press. */
function expandSelection(view: EditorView): boolean {
  const doc = view.state.doc
  const head = view.state.selection.main.head
  const block = blockRangeAt(view, head)
  if (!block) {
    // Blank line: fall through to CM's default (selects document).
    return false
  }
  // If the whole block is already selected, graduate to full-doc.
  if (selectionMatches(view, block)) {
    view.dispatch({
      selection: EditorSelection.range(0, doc.length),
      userEvent: 'select.all',
    })
    return true
  }
  view.dispatch({
    selection: EditorSelection.range(block.from, block.to),
    userEvent: 'select.block',
  })
  return true
}

/** `Shift-ArrowDown` at the end of a block → extend selection into
 *  the next block (skipping the separating blank line). Mirror for
 *  `Shift-ArrowUp` at the start. CM's default shift-arrow steps by
 *  lines; this overlays a block-level step when the caret sits at a
 *  block edge. */
function extendByBlock(dir: 'down' | 'up'): (view: EditorView) => boolean {
  return (view) => {
    const sel = view.state.selection.main
    const doc = view.state.doc
    const head = sel.head
    const line = doc.lineAt(head)
    if (line.text.trim() === '') return false
    if (dir === 'down') {
      // Only trigger when the caret is at the end of the bottom line
      // of a block. Otherwise fall through to default shift-arrow.
      if (head !== line.to) return false
      let lineNo = line.number + 1
      while (lineNo <= doc.lines && doc.line(lineNo).text.trim() === '') lineNo++
      if (lineNo > doc.lines) return false
      let endLineNo = lineNo
      while (endLineNo < doc.lines && doc.line(endLineNo + 1).text.trim() !== '') endLineNo++
      const end = doc.line(endLineNo)
      view.dispatch({
        selection: EditorSelection.range(sel.anchor, end.to),
        scrollIntoView: true,
        userEvent: 'select.block.extend',
      })
      return true
    }
    if (head !== line.from) return false
    let lineNo = line.number - 1
    while (lineNo >= 1 && doc.line(lineNo).text.trim() === '') lineNo--
    if (lineNo < 1) return false
    let startLineNo = lineNo
    while (startLineNo > 1 && doc.line(startLineNo - 1).text.trim() !== '') startLineNo--
    const start = doc.line(startLineNo)
    view.dispatch({
      selection: EditorSelection.range(sel.anchor, start.from),
      scrollIntoView: true,
      userEvent: 'select.block.extend',
    })
    return true
  }
}

export function blockSelectionExt(): Extension {
  return keymap.of([
    { key: 'Mod-a', run: expandSelection },
    { key: 'Shift-ArrowDown', run: extendByBlock('down') },
    { key: 'Shift-ArrowUp', run: extendByBlock('up') },
  ])
}
