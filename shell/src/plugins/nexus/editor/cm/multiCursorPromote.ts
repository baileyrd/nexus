// BL-051 — promote a multi-block selection into a CM6 multi-cursor.
//
// When the active selection spans more than one "block" (a maximal
// run of non-blank lines, matching `blockSelection.ts`'s definition),
// `Mod-Shift-l` collapses it into one cursor per spanned block, each
// parked at the same line/column offset its anchor's block carries.
// Editor-only; no kernel surface — the kernel never sees a multi-
// cursor selection (each subsequent transaction is dispatched as
// usual and the apply path ignores the extra heads).
//
// "Same offset" is computed as `(line-within-block, column)` rather
// than a flat byte offset so a long block + a short block don't
// collapse onto the same row. When the target block has fewer lines
// than the source's row index, the cursor lands on the last line;
// when the target line is shorter than the column, the cursor
// clamps to that line's end.

import {
  EditorSelection,
  EditorState,
  type Extension,
  type Text,
} from '@codemirror/state'
import { keymap, type EditorView } from '@codemirror/view'

/** Inclusive line-range of a block, plus its `from`/`to` doc
 *  offsets. Mirrors the in-module helper from `blockSelection.ts`
 *  but exported so this module can reuse the shape without
 *  reaching across files. */
export interface BlockRange {
  topLineNo: number
  bottomLineNo: number
  from: number
  to: number
}

/** Walk `doc` for the maximal run of non-blank lines containing
 *  `lineNo`. Returns `null` when `lineNo` itself is blank. */
export function blockOfLine(doc: Text, lineNo: number): BlockRange | null {
  if (lineNo < 1 || lineNo > doc.lines) return null
  if (doc.line(lineNo).text.trim() === '') return null
  let topLineNo = lineNo
  while (topLineNo > 1 && doc.line(topLineNo - 1).text.trim() !== '') topLineNo--
  let bottomLineNo = lineNo
  while (
    bottomLineNo < doc.lines &&
    doc.line(bottomLineNo + 1).text.trim() !== ''
  ) {
    bottomLineNo++
  }
  return {
    topLineNo,
    bottomLineNo,
    from: doc.line(topLineNo).from,
    to: doc.line(bottomLineNo).to,
  }
}

/** Enumerate every block overlapped by the doc-offset range
 *  `[from, to]`. Blocks are returned in document order; blank
 *  lines between blocks are skipped (they're not part of any
 *  block in the `blockSelection.ts` model). */
export function blocksInRange(doc: Text, from: number, to: number): BlockRange[] {
  const lo = Math.min(from, to)
  const hi = Math.max(from, to)
  const startLineNo = doc.lineAt(lo).number
  const endLineNo = doc.lineAt(hi).number
  const out: BlockRange[] = []
  let lineNo = startLineNo
  while (lineNo <= endLineNo) {
    const block = blockOfLine(doc, lineNo)
    if (block) {
      out.push(block)
      lineNo = block.bottomLineNo + 1
    } else {
      lineNo++
    }
  }
  // Edge case: `lo` started inside a blank gap but `hi` reaches a
  // block — the walk above already covers it. The opposite case
  // (selection ends inside a blank gap after a block) is also
  // handled because the trailing blank lines fail `blockOfLine`
  // and the loop exits cleanly.
  return out
}

/** Pure: compute a list of `(line, col)` cursor targets, one per
 *  block in `blocks`, with each cursor parked at the same
 *  `(rowOffset, col)` the anchor block carries.
 *
 *    rowOffset = anchorLine - anchorBlock.topLineNo
 *    col       = anchorPos - anchorLine.from
 *
 *  When a target block has fewer rows than `rowOffset` the cursor
 *  parks on its last row; when that row is shorter than `col`,
 *  it clamps to the row end. Result offsets are returned as flat
 *  doc positions so the caller can hand them straight to
 *  `EditorSelection.create`. */
export function cursorsFromBlocks(
  doc: Text,
  blocks: BlockRange[],
  anchorPos: number,
): number[] {
  const anchorLine = doc.lineAt(anchorPos)
  const anchorBlock = blockOfLine(doc, anchorLine.number)
  if (!anchorBlock) {
    // Anchor on a blank line — fallback: place a cursor at each
    // block's start line.
    return blocks.map((b) => doc.line(b.topLineNo).from)
  }
  const rowOffset = anchorLine.number - anchorBlock.topLineNo
  const col = anchorPos - anchorLine.from
  return blocks.map((b) => {
    const blockHeight = b.bottomLineNo - b.topLineNo + 1
    const targetLineNo = b.topLineNo + Math.min(rowOffset, blockHeight - 1)
    const targetLine = doc.line(targetLineNo)
    const clampedCol = Math.min(col, targetLine.length)
    return targetLine.from + clampedCol
  })
}

/** Command: promote the active selection into a multi-cursor when
 *  it spans more than one block. Returns `true` to swallow the
 *  keybinding, `false` to fall through to the next handler when
 *  the selection covers only one block (so `Mod-Shift-l`'s default
 *  CM behaviour — `selectMatches`-style — can run if added later). */
export function promoteBlockSelectionToMultiCursor(view: EditorView): boolean {
  const sel = view.state.selection.main
  if (sel.from === sel.to) return false
  const doc = view.state.doc
  const blocks = blocksInRange(doc, sel.from, sel.to)
  if (blocks.length < 2) return false

  const cursors = cursorsFromBlocks(doc, blocks, sel.anchor)
  if (cursors.length === 0) return false

  view.dispatch({
    selection: EditorSelection.create(
      cursors.map((c) => EditorSelection.cursor(c)),
      cursors.length - 1,
    ),
    userEvent: 'select.multicursor.fromBlocks',
  })
  return true
}

/** Keymap entry — `Mod-Shift-l` mirrors VS Code's "select all
 *  occurrences" chord. We bind it to the block-selection
 *  promotion because that's the closest semantic equivalent in a
 *  Notion-style block editor. CM requires
 *  `EditorState.allowMultipleSelections` to be `true` before any
 *  multi-range selection survives `view.dispatch` — bundle it
 *  into the extension so callers don't have to remember. */
export function multiCursorPromoteExt(): Extension {
  return [
    EditorState.allowMultipleSelections.of(true),
    keymap.of([
      {
        key: 'Mod-Shift-l',
        run: promoteBlockSelectionToMultiCursor,
      },
    ]),
  ]
}
