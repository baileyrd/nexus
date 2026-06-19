import { EditorView, keymap, lineNumbers } from '@codemirror/view'
import { EditorState, type Extension } from '@codemirror/state'
// Phase 5 ripped out `@codemirror/commands` `history()` + `historyKeymap`.
// The kernel's UndoTree owns history now — see
// docs/editor-transaction-wiring-plan.md §Phase 5 / resolved decision #3.
// Local undo in CM would compete with the authoritative stack and would
// not cover AI-generated edits applied via `apply_transaction`.
import { defaultKeymap } from '@codemirror/commands'
import { search, searchKeymap } from '@codemirror/search'
import { syntaxHighlighting, defaultHighlightStyle } from '@codemirror/language'

import type { EditorKernelClient } from '../kernelClient.ts'
import { clientLogger } from '../../../../clientLogger'
import { vimKeymapExt, type VimKeymapOptions } from './vimKeymap'
import { emacsKeymapExt, type EmacsKeymapOptions } from './emacsKeymap'
import { nexusSyntaxHighlighting } from './syntaxHighlight'

/** BL-070 / BL-071: opt-in keybinding layers. */
export type EditorKeybindings = 'default' | 'vim' | 'emacs'

export interface BaselineExtensionsOptions {
  /**
   * Show line numbers in the gutter. There is no settings plumbing for
   * this yet (see Phase 2 plan); callers pass an explicit value and
   * the factory defaults to `false` to avoid inventing one.
   */
  lineNumbers?: boolean
  /**
   * Soft-wrap long lines. Defaults to `true` (the prior always-wrapped
   * behaviour). Set `false` to let lines run off the right edge with a
   * horizontal scrollbar.
   */
  wordWrap?: boolean
  /**
   * Number of columns a tab character renders as (CM6 `tabSize` facet).
   * Defaults to `4` to match the CodeMirror default.
   */
  tabSize?: number
  /**
   * Optional kernel-undo binding. When present, Ctrl/Cmd-Z routes to
   * `kernelClient.undo(relpath)` and Ctrl-Y / Cmd-Shift-Z to
   * `kernelClient.redo(relpath)`. When absent (untitled tabs with no
   * session), both keys are no-ops at the extension layer — the
   * `defaultKeymap` still fires for everything else.
   */
  kernelUndo?: KernelUndoBinding
  /**
   * BL-070: when set to `'vim'`, layer
   * [`vim()`](https://www.npmjs.com/package/@replit/codemirror-vim)
   * over the default keymap. Requires `vim` to carry the per-tab
   * `relpath` + ex-command callbacks.
   */
  keybindings?: EditorKeybindings
  /**
   * Per-tab callbacks for the Vim layer's ex commands. Required when
   * `keybindings === 'vim'`; ignored otherwise.
   */
  vim?: VimKeymapOptions
  /**
   * Per-tab metadata for the Emacs layer. Required when
   * `keybindings === 'emacs'`; ignored otherwise.
   */
  emacs?: EmacsKeymapOptions
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
         
        clientLogger.error(`[nexus.editor] ${m}:`, e)
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
    // CM6 ships language parsers but no highlight style — without this
    // the parse tree is unstyled and code-mode tabs render as plain text.
    syntaxHighlighting(defaultHighlightStyle),
    search({ top: true }),
    keymap.of([...searchKeymap, ...keys]),
    EditorState.tabSize.of(opts.tabSize ?? 4),
    nexusSyntaxHighlighting,
  ]
  // Soft-wrap unless the user turned it off — default `true` preserves
  // the prior always-wrapped behaviour.
  if (opts.wordWrap !== false) exts.push(EditorView.lineWrapping)
  if (opts.lineNumbers) exts.push(lineNumbers())
  // Vim layers in front of the search/default keymaps so its modal
  // dispatch takes precedence — Normal-mode `/` reaches the vim
  // search layer before the default `searchKeymap`'s `Ctrl-F`-shaped
  // shortcuts can claim it.
  if (opts.keybindings === 'vim' && opts.vim) {
    exts.unshift(vimKeymapExt(opts.vim))
  }
  // Emacs layers in front of the default keymap so the Nexus-side
  // overrides (`C-k` with kill-ring routing, `C-w` / `M-w` / `C-y`)
  // win against any default that might claim the same chord.
  if (opts.keybindings === 'emacs' && opts.emacs) {
    exts.unshift(emacsKeymapExt(opts.emacs))
  }
  return exts
}
