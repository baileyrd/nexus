import { forwardRef, useEffect, useImperativeHandle, useRef } from 'react'
import { Compartment, EditorState, type Extension } from '@codemirror/state'
import { EditorView } from '@codemirror/view'
import { baselineExtensions, type KernelUndoBinding } from './extensions'

export interface CodeMirrorHostProps {
  value: string
  onChange: (v: string) => void
  readOnly?: boolean
  /** Show gutter line numbers. Defaults to `false`. */
  lineNumbers?: boolean
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
  className?: string
  style?: React.CSSProperties
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
      buildExtensions,
      kernelUndo,
      className,
      style,
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
    // (readOnly, lineNumbers) is synchronised via the effects below.
    useEffect(() => {
      const parent = hostRef.current
      if (!parent) return
      const extra = buildExtensionsRef.current?.() ?? []
      const state = EditorState.create({
        doc: value,
        extensions: [
          baselineCompartment.current.of(
            baselineExtensions({ lineNumbers, kernelUndo: kernelUndoRef.current }),
          ),
          readOnlyCompartment.current.of([
            EditorView.editable.of(!readOnly),
            EditorState.readOnly.of(readOnly),
          ]),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) onChangeRef.current(u.state.doc.toString())
          }),
          ...extra,
        ],
      })
      const view = new EditorView({ state, parent })
      viewRef.current = view
      return () => {
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
    // preserves doc/selection/scroll state.
    useEffect(() => {
      const view = viewRef.current
      if (!view) return
      view.dispatch({
        effects: baselineCompartment.current.reconfigure(
          baselineExtensions({ lineNumbers, kernelUndo: kernelUndoRef.current }),
        ),
      })
    }, [lineNumbers])

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
