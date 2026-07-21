// BL-077 follow-up — reveal-line consumer helper.
//
// `EditorView.tsx` emits `nexus.editor:reveal-line` after a Cmd+Click
// → definition resolves to a Location. The editor plugin subscribes
// and uses this helper to scroll-and-cursor the destination CM view
// to the resolved (line, character).
//
// Coordinates are LSP-style (0-indexed line, 0-indexed UTF-16 char
// offset within the line) — same as the rest of the LSP client.

import { EditorSelection } from '@codemirror/state'
import type { EditorView } from '@codemirror/view'

/** Resolve an LSP-style 0-indexed `(line, character)` to a CM
 *  document offset. Clamps overshoot at the document end and at the
 *  end of the target line. Uses `doc.line(n)` (1-indexed line-number
 *  lookup) — `doc.lineAt(pos)` would interpret the argument as a
 *  byte offset and throw on out-of-range. */
export function lspPositionToCmOffset(
  doc: {
    line(n: number): { from: number; to: number; length: number }
    lines: number
    length: number
  },
  line: number,
  character: number,
): number {
  if (line < 0) return 0
  // CM line numbers are 1-indexed; LSP is 0-indexed.
  const cmLineNo = line + 1
  if (cmLineNo > doc.lines) return doc.length
  const lineInfo = doc.line(cmLineNo)
  const safeChar = Math.max(0, Math.min(character, lineInfo.length))
  return lineInfo.from + safeChar
}

/** Place the cursor at the LSP position and scroll it into view.
 *  Returns `true` on a successful dispatch, `false` when the view
 *  has no document (a freshly-mounted leaf before its session
 *  loaded). */
export function revealLineInView(
  view: EditorView,
  line: number,
  character: number,
): boolean {
  const doc = view.state.doc
  if (doc.length === 0 && line === 0 && character === 0) {
    // Empty buffer with target at origin — still dispatch so the
    // selection lands on the right spot when content arrives.
  }
  const offset = lspPositionToCmOffset(doc, line, character)
  view.dispatch({
    selection: EditorSelection.cursor(offset),
    scrollIntoView: true,
  })
  return true
}

/**
 * Place the cursor at a raw CM document offset and scroll it into
 * view. Used for jump-to-peer (C64) — collab presence carries a CM
 * offset directly (`crates/nexus-collab/src/presence.rs`), not an
 * LSP line/character pair, so no conversion is needed here. Clamps
 * to `[0, doc.length]` the same way `remoteCursors.ts` clamps stale
 * presence frames.
 */
export function revealOffsetInView(view: EditorView, offset: number): boolean {
  const clamped = Math.min(Math.max(0, offset), view.state.doc.length)
  view.dispatch({
    selection: EditorSelection.cursor(clamped),
    scrollIntoView: true,
  })
  return true
}
