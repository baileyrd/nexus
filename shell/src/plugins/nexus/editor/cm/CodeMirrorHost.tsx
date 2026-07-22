import { forwardRef, useEffect, useImperativeHandle, useRef } from 'react'
import { Compartment, EditorSelection, EditorState, type Extension } from '@codemirror/state'
import { EditorView } from '@codemirror/view'
import {
  baselineExtensions,
  type EditorKeybindings,
  type KernelUndoBinding,
} from './extensions'
import type { VimKeymapOptions } from './vimKeymap'
import type { EmacsKeymapOptions } from './emacsKeymap'

export interface CodeMirrorHostProps {
  value: string
  onChange: (v: string) => void
  readOnly?: boolean
  /** Show gutter line numbers. Defaults to `false`. */
  lineNumbers?: boolean
  /** Soft-wrap long lines. Defaults to `true`. */
  wordWrap?: boolean
  /** Tab width in columns (CM6 `tabSize` facet). Defaults to `4`. */
  tabSize?: number
  /** #357: enable the webview's native spellchecker. Defaults to `true`. */
  spellcheck?: boolean
  /** BCP-47 language tag hinting the spellchecker's dictionary. Defaults to `en-US`. */
  spellcheckLanguage?: string
  /**
   * Extra CM extensions appended after the baseline. The Phase 5
   * transaction bridge threads through here — the host stays generic
   * and doesn't know about sessions or the kernel. For undo/redo
   * keybinding, use `kernelUndo` instead, which plugs into the
   * baseline keymap.
   *
   * Passed as a factory so callers can close over per-mount state
   * (e.g. the relpath) without forcing the host to resubscribe on
   * every render. The factory is invoked once per mount; prop changes
   * do not rebuild the view.
   */
  buildExtensions?: () => Extension[]
  /**
   * Kernel-backed undo/redo binding. When set, Ctrl/Cmd-Z → undo,
   * Ctrl-Y / Cmd-Shift-Z → redo via the `EditorKernelClient`. Absent
   * for untitled tabs (no session) — those chords become no-ops at
   * the bridge layer; the host doesn't fall back to local history.
   */
  kernelUndo?: KernelUndoBinding
  /** BL-070: which keybinding layer to mount on top of the defaults. */
  keybindings?: EditorKeybindings
  /** Required when `keybindings === 'vim'`; ignored otherwise. */
  vim?: VimKeymapOptions
  /** Required when `keybindings === 'emacs'`; ignored otherwise. */
  emacs?: EmacsKeymapOptions
  className?: string
  style?: React.CSSProperties
  /**
   * #405 — character offset to place the cursor at on mount (clamped
   * to the initial `value`'s length). Read once at mount time, same
   * as every other construction-only prop here — a later change does
   * not move the live cursor; callers who want that should remount
   * via `key`.
   */
  initialSelection?: number
  /** #405 — `scrollDOM.scrollTop` to restore once the view has laid
   *  out. Applied one frame after mount so the scroll container has
   *  real dimensions. */
  initialScrollTop?: number
  /**
   * #405 — debounced (300ms) callback firing the current cursor
   * offset + `scrollDOM.scrollTop` on every selection change or
   * scroll, so callers can persist position for tabs that stay
   * mounted-but-hidden (the workspace never unmounts a backgrounded
   * tab, just toggles `display: none`) rather than only capturing on
   * unmount.
   */
  onPositionChange?: (offset: number, scrollTop: number) => void
}

/**
 * Handle exposed to parents so they can scroll to a line, focus, or
 * otherwise reach into the `EditorView` without this component owning
 * every imperative affordance. Intentionally narrow: we only expose
 * the raw view; helpers (like `viewToLine`) live at the call site so
 * they can evolve without churning this wrapper.
 */
export interface CodeMirrorHostHandle {
  view: EditorView | null
}

/**
 * Imperative React wrapper around a CodeMirror 6 `EditorView`.
 *
 * Lifecycle:
 *   - The view is constructed once in a mount-only `useEffect` and
 *     destroyed on unmount. We never rebuild on prop changes — the
 *     view is long-lived and mutated via transactions.
 *   - `value` prop acts as a one-way mirror of external state. When
 *     it diverges from `view.state.doc.toString()` we replace the
 *     whole doc. The echo from our own `updateListener` always
 *     matches current doc, so it won't retrigger the dispatch.
 *   - `onChange` fires on every doc change; the callback ref keeps
 *     us from rebuilding the view when the parent passes a new
 *     closure each render.
 *   - `readOnly` toggles via a compartment-free `setState`-style
 *     reconfigure: we reconstruct the baseline extensions when it
 *     flips. Baseline rebuilds are cheap and phase-2-scoped;
 *     compartments can replace this later.
 */
export const CodeMirrorHost = forwardRef<CodeMirrorHostHandle, CodeMirrorHostProps>(
  function CodeMirrorHost(
    {
      value,
      onChange,
      readOnly = false,
      lineNumbers = false,
      wordWrap = true,
      tabSize = 4,
      spellcheck = true,
      spellcheckLanguage = 'en-US',
      buildExtensions,
      kernelUndo,
      keybindings,
      vim,
      emacs,
      className,
      style,
      initialSelection,
      initialScrollTop,
      onPositionChange,
    },
    ref,
  ) {
    // Stash the factory in a ref so prop churn never resubscribes — the
    // view is built exactly once per mount. Callers who want to swap
    // the extension set remount the host (e.g. by keying on relpath).
    const buildExtensionsRef = useRef(buildExtensions)
    buildExtensionsRef.current = buildExtensions
    const kernelUndoRef = useRef(kernelUndo)
    kernelUndoRef.current = kernelUndo
    const hostRef = useRef<HTMLDivElement | null>(null)
    const viewRef = useRef<EditorView | null>(null)
    // Compartments let us swap baseline / readOnly extensions without
    // tearing down the view (which would lose cursor, scroll, undo).
    const baselineCompartment = useRef(new Compartment())
    const readOnlyCompartment = useRef(new Compartment())
    // Stable callback ref so the updateListener doesn't close over a
    // stale `onChange` and so we don't rebuild the view on re-renders.
    const onChangeRef = useRef(onChange)
    onChangeRef.current = onChange
    // #405 — read once at mount (construction-only, like every other
    // prop this component doesn't react to post-mount).
    const initialSelectionRef = useRef(initialSelection)
    const initialScrollTopRef = useRef(initialScrollTop)
    const onPositionChangeRef = useRef(onPositionChange)
    onPositionChangeRef.current = onPositionChange
    const positionCaptureTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

    useImperativeHandle(
      ref,
      () => ({
        get view() {
          return viewRef.current
        },
      }),
      [],
    )

    // Mount the view exactly once. Any prop that influences construction
    // (readOnly, lineNumbers, wordWrap, tabSize) is synchronised via the
    // effects below.
    useEffect(() => {
      const parent = hostRef.current
      if (!parent) return
      const extra = buildExtensionsRef.current?.() ?? []
      // #405 — clamp to the initial doc so a stale persisted offset
      // (content changed on disk since the position was captured)
      // can't throw constructing the selection.
      const initialOffset =
        initialSelectionRef.current != null
          ? Math.min(Math.max(0, initialSelectionRef.current), value.length)
          : undefined
      // #405 — debounced capture shared by the selection-change branch
      // of the updateListener below and the scroll listener attached
      // after the view exists.
      const capturePosition = (view: EditorView) => {
        if (positionCaptureTimer.current != null) clearTimeout(positionCaptureTimer.current)
        positionCaptureTimer.current = setTimeout(() => {
          positionCaptureTimer.current = null
          onPositionChangeRef.current?.(view.state.selection.main.head, view.scrollDOM.scrollTop)
        }, 300)
      }
      const state = EditorState.create({
        doc: value,
        selection: initialOffset != null ? EditorSelection.cursor(initialOffset) : undefined,
        extensions: [
          baselineCompartment.current.of(
            baselineExtensions({
              lineNumbers,
              wordWrap,
              tabSize,
              spellcheck,
              spellcheckLanguage,
              kernelUndo: kernelUndoRef.current,
              keybindings,
              vim,
              emacs,
            }),
          ),
          readOnlyCompartment.current.of([
            EditorView.editable.of(!readOnly),
            EditorState.readOnly.of(readOnly),
          ]),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) onChangeRef.current(u.state.doc.toString())
            if (u.docChanged || u.selectionSet) capturePosition(u.view)
          }),
          ...extra,
        ],
      })
      const view = new EditorView({ state, parent })
      viewRef.current = view

      // #405 — restore scroll one frame after mount so `scrollDOM` has
      // real layout dimensions (setting it synchronously at mount can
      // no-op if the container hasn't been measured yet).
      let scrollRestoreFrame = 0
      if (initialScrollTopRef.current != null) {
        scrollRestoreFrame = requestAnimationFrame(() => {
          if (viewRef.current) viewRef.current.scrollDOM.scrollTop = initialScrollTopRef.current!
        })
      }
      const onScroll = () => capturePosition(view)
      view.scrollDOM.addEventListener('scroll', onScroll, { passive: true })

      return () => {
        cancelAnimationFrame(scrollRestoreFrame)
        view.scrollDOM.removeEventListener('scroll', onScroll)
        if (positionCaptureTimer.current != null) clearTimeout(positionCaptureTimer.current)
        // Final synchronous flush so a tab close / app quit right
        // after the last edit doesn't lose up to 300ms of position.
        onPositionChangeRef.current?.(view.state.selection.main.head, view.scrollDOM.scrollTop)
        view.destroy()
        viewRef.current = null
      }
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])

    // Mirror external doc changes into CM. Guard against the echo from
    // our own updateListener by comparing to the current doc first.
    useEffect(() => {
      const view = viewRef.current
      if (!view) return
      const current = view.state.doc.toString()
      if (current === value) return
      view.dispatch({
        changes: { from: 0, to: current.length, insert: value },
      })
    }, [value])

    // Reconfigure compartments when gated options flip. Cheap and
    // preserves doc/selection/scroll state. `keybindings`/`vim` are
    // intentionally not in the dep list — the parent (`EditorView`)
    // remounts the host via its `key` when those change, because the
    // vim layer's modal state can't be cleanly hot-swapped in place.
    useEffect(() => {
      const view = viewRef.current
      if (!view) return
      view.dispatch({
        effects: baselineCompartment.current.reconfigure(
          baselineExtensions({
              lineNumbers,
              wordWrap,
              tabSize,
              spellcheck,
              spellcheckLanguage,
              kernelUndo: kernelUndoRef.current,
              keybindings,
              vim,
              emacs,
            }),
        ),
      })
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [lineNumbers, wordWrap, tabSize, spellcheck, spellcheckLanguage])

    useEffect(() => {
      const view = viewRef.current
      if (!view) return
      view.dispatch({
        effects: readOnlyCompartment.current.reconfigure([
          EditorView.editable.of(!readOnly),
          EditorState.readOnly.of(readOnly),
        ]),
      })
    }, [readOnly])

    return <div ref={hostRef} className={className} style={style} />
  },
)
