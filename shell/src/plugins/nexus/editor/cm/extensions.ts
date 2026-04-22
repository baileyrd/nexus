import { EditorView, keymap, lineNumbers } from '@codemirror/view'
import type { Extension } from '@codemirror/state'
import { markdown } from '@codemirror/lang-markdown'
// Phase 5 ripped out `@codemirror/commands` `history()` + `historyKeymap`.
// The kernel's UndoTree owns history now — see
// docs/editor-transaction-wiring-plan.md §Phase 5 / resolved decision #3.
// Local undo in CM would compete with the authoritative stack and would
// not cover AI-generated edits applied via `apply_transaction`.
import { defaultKeymap } from '@codemirror/commands'
import { search, searchKeymap } from '@codemirror/search'

import type { EditorKernelClient } from '../kernelClient.ts'

export interface BaselineExtensionsOptions {
  /**
   * Show line numbers in the gutter. There is no settings plumbing for
   * this yet (see Phase 2 plan); callers pass an explicit value and
   * the factory defaults to `false` to avoid inventing one.
   */
  lineNumbers?: boolean
  /**
   * Optional kernel-undo binding. When present, Ctrl/Cmd-Z routes to
   * `kernelClient.undo(relpath)` and Ctrl-Y / Cmd-Shift-Z to
   * `kernelClient.redo(relpath)`. When absent (untitled tabs with no
   * session), both keys are no-ops at the extension layer — the
   * `defaultKeymap` still fires for everything else.
   */
  kernelUndo?: KernelUndoBinding
}

/** Binding options for the kernel-backed undo/redo keymap. */
export interface KernelUndoBinding {
  relpath: string
  kernelClient: EditorKernelClient
  /**
   * Called with the authoritative markdown after an undo/redo so the
   * caller can replace the CM doc. The bridge wires this to the same
   * `reconcileFromCanonical` helper it uses for `apply_transaction`
   * responses; callers without the bridge can pass a minimal replace.
   */
  applyCanonical: (view: EditorView, canonical: string) => void
  /** Error sink; defaults to `console.error`. */
  onError?: (message: string, err: unknown) => void
}

/**
 * Baseline CodeMirror extension set used by `CodeMirrorHost`. Kept in
 * its own module so later phases can layer on session-driven
 * transactions, decorations, and the real undo/redo binding without
 * touching the host component.
 */
export function baselineExtensions(
  opts: BaselineExtensionsOptions = {},
): Extension[] {
  const keys = [...defaultKeymap]

  if (opts.kernelUndo) {
    const {
      relpath,
      kernelClient,
      applyCanonical,
      onError = (m, e) => {
        // eslint-disable-next-line no-console
        console.error(`[nexus.editor] ${m}:`, e)
      },
    } = opts.kernelUndo

    const runUndo = (view: EditorView): boolean => {
      void kernelClient
        .undo(relpath)
        .then(() => kernelClient.getMarkdown(relpath))
        .then((canonical) => applyCanonical(view, canonical))
        .catch((err) => onError('editor bridge: undo failed', err))
      return true
    }

    const runRedo = (view: EditorView): boolean => {
      void kernelClient
        .redo(relpath)
        .then(() => kernelClient.getMarkdown(relpath))
        .then((canonical) => applyCanonical(view, canonical))
        .catch((err) => onError('editor bridge: redo failed', err))
      return true
    }

    // Prepend so our bindings win over any later default that might
    // happen to claim the same chord. `preventDefault: true` stops the
    // browser's built-in undo.
    keys.unshift(
      { key: 'Mod-z', preventDefault: true, run: runUndo },
      { key: 'Mod-y', preventDefault: true, run: runRedo },
      { key: 'Mod-Shift-z', preventDefault: true, run: runRedo },
    )
  }

  const exts: Extension[] = [
    markdown(),
    search({ top: true }),
    keymap.of([...searchKeymap, ...keys]),
    EditorView.lineWrapping,
  ]
  if (opts.lineNumbers) exts.push(lineNumbers())
  return exts
}
