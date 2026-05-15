// shell/src/plugins/nexus/editor/cm/breakpointGutter.ts
//
// BL-081 follow-up — clickable breakpoint gutter for code-mode tabs.
//
// Mirrors the `gitGutter` shape: state field holds the per-line marker
// set, a ViewPlugin syncs the field with the debugger store via
// `subscribe`, and a CM6 `gutter()` renders a red dot per breakpoint
// line and routes click events to the `onToggle` callback.
//
// The toggle callback is wired by the editor host to
// `useDebuggerStore.getState().toggleBreakpoint(api, relpath, line)`
// — that path already (a) updates the store's per-source breakpoint
// list and (b) dispatches `set_breakpoints` to the adapter when a
// session is active. Clicking a row without an active session still
// records the breakpoint so the next `launch` replays it (the store
// re-issues every cached entry post-`launch`, pre-`configurationDone`).

import {
  StateEffect,
  StateField,
  type Extension,
} from '@codemirror/state'
import {
  EditorView,
  GutterMarker,
  ViewPlugin,
  type ViewUpdate,
  gutter,
} from '@codemirror/view'

/** Lines that currently carry a breakpoint, 1-based. */
export interface BreakpointGutterState {
  lines: Set<number>
}

const setBreakpointLines = StateEffect.define<BreakpointGutterState>()

const breakpointStateField = StateField.define<BreakpointGutterState>({
  create: () => ({ lines: new Set() }),
  update(value, tr) {
    for (const e of tr.effects) {
      if (e.is(setBreakpointLines)) return e.value
    }
    return value
  },
})

class BreakpointMarker extends GutterMarker {
  toDOM() {
    const el = document.createElement('div')
    el.className = 'nexus-debugger-gutter-marker nexus-debugger-gutter-bp'
    el.title = 'Breakpoint — click to remove'
    return el
  }
}

const BREAKPOINT = new BreakpointMarker()

/**
 * Pure factor — derive the line set the gutter should render given a
 * snapshot of the debugger store's per-source breakpoint map and the
 * file's relpath. Exported so tests can drive the conversion without
 * a CM6 view.
 */
export function linesForPath(
  breakpointsByPath: Record<string, ReadonlyArray<{ line: number }>>,
  relpath: string,
): Set<number> {
  const entries = breakpointsByPath[relpath]
  if (!entries) return new Set()
  return new Set(entries.map((b) => b.line))
}

/**
 * Pure factor — compare two line sets for equality without iterating
 * twice on the hot path. The gutter only dispatches a state update
 * when the set actually changes, so the editor's transaction stream
 * stays quiet on no-op store mutations.
 */
export function lineSetsEqual(a: Set<number>, b: Set<number>): boolean {
  if (a.size !== b.size) return false
  for (const n of a) {
    if (!b.has(n)) return false
  }
  return true
}

/** Subscribe-shape the gutter consumes — Zustand's `subscribe` matches. */
export interface BreakpointStoreSubscriber {
  /** Initial snapshot (used at mount). */
  getSnapshot(): Record<string, ReadonlyArray<{ line: number }>>
  /** Subscribe; returns unsubscribe. Listener fires on every store update. */
  subscribe(listener: () => void): () => void
}

export interface BreakpointGutterDeps {
  /** Forge-relative path of the buffer this gutter is wired to. */
  relpath: string
  /** Snapshot + subscribe surface — pass `useDebuggerStore` as-is in production. */
  store: BreakpointStoreSubscriber
  /**
   * Toggle a breakpoint at `line` for `relpath`. Fire-and-forget; the
   * store's own state update will fan out through `subscribe` so the
   * gutter re-renders without the caller having to thread the new line
   * back in.
   */
  onToggle(relpath: string, line: number): void
}

/**
 * BL-081 follow-up — root extension wiring a clickable breakpoint
 * gutter for `deps.relpath`. Mount once per code-mode editor view.
 */
export function breakpointGutterExt(deps: BreakpointGutterDeps): Extension {
  const refresh = (view: EditorView) => {
    const snap = deps.store.getSnapshot()
    const lines = linesForPath(snap, deps.relpath)
    const current = view.state.field(breakpointStateField, false)
    if (current && lineSetsEqual(current.lines, lines)) return
    view.dispatch({ effects: setBreakpointLines.of({ lines }) })
  }

  // CM6 disallows `view.dispatch` from inside a ViewPlugin constructor,
  // so the initial snapshot is fed through `StateField.init(...)` which
  // overrides the field's `create` for this view. Subsequent store
  // notifications dispatch the effect normally from the watcher below.
  const seed = breakpointStateField.init(() => ({
    lines: linesForPath(deps.store.getSnapshot(), deps.relpath),
  }))

  const watcher = ViewPlugin.fromClass(
    class {
      private readonly view: EditorView
      private unsub: (() => void) | null = null

      constructor(view: EditorView) {
        this.view = view
        this.unsub = deps.store.subscribe(() => refresh(this.view))
      }

      update(_u: ViewUpdate) {
        // Doc-change-driven refresh is unnecessary — only the store
        // can mint or drop a breakpoint, and `subscribe` covers that.
      }

      destroy() {
        this.unsub?.()
        this.unsub = null
      }
    },
  )

  return [
    breakpointStateField,
    seed,
    watcher,
    gutter({
      class: 'nexus-debugger-gutter',
      lineMarker(view, blockInfo) {
        const state = view.state.field(breakpointStateField, false)
        if (!state) return null
        const lineObj = view.state.doc.lineAt(blockInfo.from)
        return state.lines.has(lineObj.number) ? BREAKPOINT : null
      },
      // An empty spacer reserves layout width; without it the gutter
      // collapses to 0 px when no breakpoints exist and clicks in the
      // empty column miss the gutter entirely.
      initialSpacer: () => BREAKPOINT,
      domEventHandlers: {
        click(view, blockInfo, _event) {
          const lineObj = view.state.doc.lineAt(blockInfo.from)
          deps.onToggle(deps.relpath, lineObj.number)
          return true
        },
      },
    }),
  ]
}

/** Public re-export so callers (tests, future toggle UIs) can read or
 *  poke the field without taking a deep import path on this module. */
export { setBreakpointLines, breakpointStateField }
