// BL-142 Phase 2b.2 — clickable Run gutter for REPL-flagged code
// fences (` ```python repl `). Mirrors the shape of `breakpointGutter.ts`:
//
// - A `StateField` holds the set of fence-open lines that should
//   show the marker.
// - A `ViewPlugin` watches doc changes and refreshes the state
//   field whenever the underlying REPL block layout changes.
// - A `gutter()` renders a `▶` marker per fence-open line and
//   routes click events to the caller-supplied `onRun` callback.
//
// Dependency injection: the gutter doesn't know about the kernel,
// the REPL store, or the kernel-config schema. EditorView.tsx
// builds an `onRun(line)` closure that does
// `useReplStore.evalCode(...)` with the right (relpath, lang, code).

import { StateEffect, StateField, type Extension } from '@codemirror/state'
import {
  EditorView,
  GutterMarker,
  ViewPlugin,
  type ViewUpdate,
  gutter,
} from '@codemirror/view'

import {
  extractBlockCode,
  findReplBlocks,
  type ReplFenceBlock,
} from './replFence.ts'

/** Lines (1-based) that should render the Run marker. */
export interface ReplGutterState {
  /** Maps openLine → the matching block; used by the click handler
   *  to surface the body and language without re-scanning the doc. */
  blocks: Map<number, ReplFenceBlock>
}

const setReplBlocks = StateEffect.define<ReplGutterState>()

const replStateField = StateField.define<ReplGutterState>({
  create: () => ({ blocks: new Map() }),
  update(value, tr) {
    for (const e of tr.effects) {
      if (e.is(setReplBlocks)) return e.value
    }
    return value
  },
})

class RunMarker extends GutterMarker {
  toDOM() {
    const el = document.createElement('div')
    el.className = 'nexus-repl-gutter-marker'
    el.title = 'Run this REPL cell (Shift-Enter inside the cell)'
    el.textContent = '▶'
    return el
  }
}

const RUN = new RunMarker()

/**
 * Derive the gutter state from the document text. Exported so
 * tests can drive the conversion without spinning up a CM6 view.
 */
export function stateFromDoc(docText: string): ReplGutterState {
  const blocks = findReplBlocks(docText)
  const map = new Map<number, ReplFenceBlock>()
  for (const b of blocks) map.set(b.openLine, b)
  return { blocks: map }
}

/** Cheap deep-equality check on the keys — equivalent gutter
 *  renderings shouldn't dispatch an effect that retriggers a paint. */
export function stateIsSame(a: ReplGutterState, b: ReplGutterState): boolean {
  if (a.blocks.size !== b.blocks.size) return false
  for (const k of a.blocks.keys()) {
    if (!b.blocks.has(k)) return false
  }
  return true
}

/** Inputs the host wires up. */
export interface ReplGutterDeps {
  /**
   * Fire-and-forget eval trigger for the REPL cell whose opening
   * fence is at `openLine`. The gutter passes `(block, code)` so
   * the host's closure can dispatch `replStore.evalCode(..., code)`
   * without re-extracting the body. Matches the `replKeymap`
   * `onRun` signature so a single handler can serve both call
   * sites.
   */
  onRun(block: ReplFenceBlock, code: string): void
}

/**
 * Root extension wiring the Run gutter. Mount once per editor view
 * that should expose REPL execution. Untitled tabs are fine — the
 * gutter just renders no markers (no `repl` fences).
 */
export function replGutterExt(deps: ReplGutterDeps): Extension {
  const refresh = (view: EditorView) => {
    const next = stateFromDoc(view.state.doc.toString())
    const cur = view.state.field(replStateField, false)
    if (cur && stateIsSame(cur, next)) return
    view.dispatch({ effects: setReplBlocks.of(next) })
  }

  const seed = replStateField.init(() => ({
    blocks: new Map(
      findReplBlocks('').map((b) => [b.openLine, b] as const),
    ),
  }))

  const watcher = ViewPlugin.fromClass(
    class {
      private readonly view: EditorView
      constructor(view: EditorView) {
        this.view = view
        // Defer the initial scan until after the first commit so
        // we don't dispatch from inside the view constructor.
        queueMicrotask(() => refresh(this.view))
      }
      update(u: ViewUpdate) {
        if (u.docChanged) refresh(u.view)
      }
    },
  )

  return [
    replStateField,
    seed,
    watcher,
    gutter({
      class: 'nexus-repl-gutter',
      lineMarker(view, blockInfo) {
        const state = view.state.field(replStateField, false)
        if (!state) return null
        const lineObj = view.state.doc.lineAt(blockInfo.from)
        return state.blocks.has(lineObj.number) ? RUN : null
      },
      // Empty spacer so clicks in the empty column still land on
      // the gutter even before any REPL block exists (same trick
      // breakpointGutter uses).
      initialSpacer: () => RUN,
      domEventHandlers: {
        click(view, blockInfo, _event) {
          const state = view.state.field(replStateField, false)
          if (!state) return false
          const lineObj = view.state.doc.lineAt(blockInfo.from)
          const block = state.blocks.get(lineObj.number)
          if (!block) return false
          const code = extractBlockCode(view.state.doc.toString(), block)
          deps.onRun(block, code)
          return true
        },
      },
    }),
  ]
}

export { setReplBlocks, replStateField }
