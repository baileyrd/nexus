import { keymap, type EditorView, type KeyBinding } from '@codemirror/view'
import { type Extension, EditorSelection } from '@codemirror/state'
import {
  cursorGroupBackward,
  cursorGroupForward,
  emacsStyleKeymap,
  selectGroupBackward,
  selectGroupForward,
} from '@codemirror/commands'

/**
 * BL-071: optional Emacs keybinding layer for the Nexus markdown
 * editor.
 *
 * Builds on top of CodeMirror's `emacsStyleKeymap`
 * (C-f / b / n / p / a / e / d / h / k / o / t / v) by adding the
 * editing chords the DoD calls out:
 *
 *   - `M-f` / `M-b`: word-forward / word-backward.
 *   - `C-Space`: set mark — pushed onto a per-view 16-slot ring.
 *   - `C-w` / `M-w`: kill / copy region to a process-global kill ring.
 *   - `C-k`: kill to line end (overrides the upstream binding so the
 *     killed text reaches the kill ring rather than being silently
 *     discarded).
 *   - `C-y`: yank the most recent kill-ring entry at the cursor.
 *
 * The `C-x C-s` save chord is intentionally not bound — Nexus owns
 * `Ctrl/Cmd-S` as an application-shell shortcut. Mark-ring rotation
 * (`C-u C-Space`) and `M-y` (yank-pop) are deferred.
 */

/** Maximum positions retained on each tab's mark ring. */
export const MARK_RING_LIMIT = 16

/**
 * Kill-ring depth (most-recent-last). Real Emacs lets the ring grow
 * unbounded; we cap it so the per-process buffer doesn't grow without
 * bound across a long-lived shell session. 60 is well above the
 * working set a single editing session generates.
 */
export const KILL_RING_LIMIT = 60

// ── Kill ring (process-global, mirrors Emacs) ────────────────────────────────

const killRing: string[] = []

/**
 * Push `text` onto the kill ring. Empty kills are dropped (matches
 * Emacs's behaviour of only recording kills with content).
 */
export function pushKill(text: string): void {
  if (text.length === 0) return
  killRing.push(text)
  while (killRing.length > KILL_RING_LIMIT) killRing.shift()
}

/**
 * Most recent kill, or `null` if the ring is empty. Exported for
 * tests; production code reaches it via the `C-y` binding.
 */
export function peekKill(): string | null {
  return killRing.length > 0 ? killRing[killRing.length - 1] : null
}

/** Test-only reset hook so unit tests don't leak kill-ring state. */
export function resetKillRingForTests(): void {
  killRing.length = 0
}

// ── Mark ring (per-view, attached as an instance property) ───────────────────
//
// Real Emacs remaps mark positions through buffer edits. CodeMirror's
// `StateField.update` could do that, but writing back to a state field
// from a key handler requires a custom `StateEffect` round-trip and
// every motion would have to re-read it. The DoD only specifies "ring
// of up to 16 positions" — for v1 we attach the ring directly to the
// view and don't track positions across doc edits. A subsequent edit
// invalidates older marks; that matches typical Emacs muscle memory
// (set-mark → motion → use mark) without the StateField complexity.

interface MarkRingHost {
  __nexusEmacsMarkRing?: number[]
}

function getRing(view: EditorView): number[] {
  const host = view as unknown as MarkRingHost
  if (!host.__nexusEmacsMarkRing) host.__nexusEmacsMarkRing = []
  return host.__nexusEmacsMarkRing
}

/** Snapshot of the view's mark ring. Test-facing. */
export function getMarkRing(view: EditorView): readonly number[] {
  return [...getRing(view)]
}

// ── Commands ─────────────────────────────────────────────────────────────────

const setMark = (view: EditorView): boolean => {
  const head = view.state.selection.main.head
  const ring = getRing(view)
  ring.push(head)
  while (ring.length > MARK_RING_LIMIT) ring.shift()
  // Re-anchor so a subsequent shifted motion extends a region from
  // the mark — matches the user-visible effect of `C-Space` in Emacs.
  view.dispatch({ selection: EditorSelection.cursor(head) })
  return true
}

const killRegion = (view: EditorView): boolean => {
  const sel = view.state.selection.main
  if (sel.empty) return false
  pushKill(view.state.doc.sliceString(sel.from, sel.to))
  view.dispatch({
    changes: { from: sel.from, to: sel.to, insert: '' },
    selection: EditorSelection.cursor(sel.from),
  })
  return true
}

const copyRegion = (view: EditorView): boolean => {
  const sel = view.state.selection.main
  if (sel.empty) return false
  pushKill(view.state.doc.sliceString(sel.from, sel.to))
  // Collapse the selection so a follow-up `C-y` lands at the cursor
  // rather than replacing the just-copied region — matches Emacs.
  view.dispatch({ selection: EditorSelection.cursor(sel.to) })
  return true
}

const yank = (view: EditorView): boolean => {
  const text = peekKill()
  if (text === null) return false
  const sel = view.state.selection.main
  view.dispatch({
    changes: { from: sel.from, to: sel.to, insert: text },
    selection: EditorSelection.cursor(sel.from + text.length),
  })
  return true
}

const killToLineEnd = (view: EditorView): boolean => {
  const sel = view.state.selection.main
  const line = view.state.doc.lineAt(sel.head)
  // Emacs `C-k`: at end-of-line, kill the newline; otherwise kill to
  // (but not including) the newline.
  const from = sel.head
  let to: number
  let killed: string
  if (sel.head === line.to) {
    if (line.to === view.state.doc.length) return false
    to = line.to + 1
    killed = '\n'
  } else {
    to = line.to
    killed = view.state.doc.sliceString(from, to)
  }
  pushKill(killed)
  view.dispatch({
    changes: { from, to, insert: '' },
    selection: EditorSelection.cursor(from),
  })
  return true
}

// ── Public extension factory ─────────────────────────────────────────────────

export interface EmacsKeymapOptions {
  /**
   * Forge-relative path of the host tab. Stashed for symmetry with the
   * vim layer; not consumed today, but a future `C-x b`-style switcher
   * needs it.
   */
  relpath: string
}

/**
 * Build the Emacs keymap extension stack for a single tab. Layered in
 * front of `emacsStyleKeymap` so our `C-k` override (which routes
 * through the kill ring) wins.
 */
export function emacsKeymapExt(_opts: EmacsKeymapOptions): Extension {
  const overrides: KeyBinding[] = [
    { key: 'Ctrl-Space', preventDefault: true, run: setMark },
    { key: 'Ctrl-w', preventDefault: true, run: killRegion },
    { key: 'Alt-w', preventDefault: true, run: copyRegion },
    { key: 'Ctrl-y', preventDefault: true, run: yank },
    { key: 'Ctrl-k', preventDefault: true, run: killToLineEnd },
    {
      key: 'Alt-f',
      preventDefault: true,
      run: cursorGroupForward,
      shift: selectGroupForward,
    },
    {
      key: 'Alt-b',
      preventDefault: true,
      run: cursorGroupBackward,
      shift: selectGroupBackward,
    },
  ]
  return keymap.of([...overrides, ...emacsStyleKeymap])
}
