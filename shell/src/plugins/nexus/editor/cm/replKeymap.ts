// BL-142 Phase 2b.2 — `Shift-Enter` keybinding that, when fired
// inside a REPL-flagged code block, sends the block's body to the
// configured kernel for that language.
//
// Outside a REPL block the binding returns `false` so CM6 falls
// through to the next handler in the keymap chain (markdown's
// default Enter, the language-mode extension's autoindent, etc.).
// That fall-through is what makes Shift-Enter safe to install
// globally rather than gating on "is this a code-mode tab".

import type { Extension } from '@codemirror/state'
import { keymap, type EditorView, type KeyBinding } from '@codemirror/view'

import {
  extractBlockCode,
  findReplBlockAtLine,
  findReplBlocks,
  type ReplFenceBlock,
} from './replFence.ts'

/**
 * Look up the REPL block containing the editor's primary cursor.
 * Returns `null` when the cursor is outside every REPL block, OR
 * when the document has no REPL blocks at all. Pure factor —
 * exported so tests can validate the cursor → block resolution
 * without spinning up CM6.
 */
export function blockForCursor(
  docText: string,
  cursorOffset: number,
): { block: ReplFenceBlock; code: string } | null {
  // Convert offset → 1-based line number the same way CM6 does.
  // We count newlines up to the cursor offset; line 1 is offsets
  // 0..first-newline.
  let line = 1
  const upTo = Math.min(cursorOffset, docText.length)
  for (let i = 0; i < upTo; i++) {
    if (docText.charCodeAt(i) === 0x0a) line++
  }
  const blocks = findReplBlocks(docText)
  const block = findReplBlockAtLine(blocks, line)
  if (!block) return null
  return { block, code: extractBlockCode(docText, block) }
}

/** Inputs the host wires up. */
export interface ReplKeymapDeps {
  /**
   * Fire-and-forget eval trigger. `block.language` + `code` are
   * passed through; the host's closure handles the (relpath, lang,
   * code) → `replStore.evalCode` plumbing.
   */
  onRun(block: ReplFenceBlock, code: string): void
}

/**
 * Build the `Shift-Enter` extension. Returns CM6's `keymap.of([...])`
 * so the caller drops it into the editor's extension array.
 *
 * `Mod-Enter` is bound alongside so macOS users with Shift-Enter
 * remapped (a common Vim setup) still have a path. Both keys
 * delegate to the same handler.
 */
export function replKeymapExt(deps: ReplKeymapDeps): Extension {
  const run = (view: EditorView): boolean => {
    const cursor = view.state.selection.main.head
    const docText = view.state.doc.toString()
    const hit = blockForCursor(docText, cursor)
    if (!hit) return false
    deps.onRun(hit.block, hit.code)
    return true
  }
  const bindings: KeyBinding[] = [
    { key: 'Shift-Enter', run },
    { key: 'Mod-Enter', run },
  ]
  return keymap.of(bindings)
}
