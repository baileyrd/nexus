// BL-142 Phase 2b.2 — CM6 decoration widget that renders REPL
// output inline below each `repl`-flagged code fence. Output text
// is read from `useReplOutputStore` (per-sessionId, ANSI-stripped
// by the bus pump). The widget subscribes to the store at mount
// and unsubscribes at destroy, updating its DOM in place to avoid
// the CM6 widget-remount churn on every chunk.
//
// Resolving "which sessionId belongs to this block" is left to the
// host (via the caller-supplied `resolveSessionId(block)` callback)
// because the cell↔session mapping lives in `useReplStore` keyed
// by (relpath, lang), and the gutter/keymap layer already owns
// that resolution.
//
// Why a StateField (not a ViewPlugin) drives the decoration set:
// CM6 requires `Decoration.widget({ block: true })` to come from a
// StateField — block-level decorations from a ViewPlugin throw
// "Block decorations may not be specified via plugins" at construction.
// A companion ViewPlugin subscribes to `useReplStore` so a freshly-
// minted session triggers a refresh effect even though the doc
// hasn't changed.

import {
  RangeSetBuilder,
  StateEffect,
  StateField,
  type EditorState,
  type Extension,
} from '@codemirror/state'
import {
  Decoration,
  DecorationSet,
  EditorView,
  ViewPlugin,
  WidgetType,
  type ViewUpdate,
} from '@codemirror/view'

import { findReplBlocks, type ReplFenceBlock } from './replFence.ts'
import { useReplOutputStore } from '../replOutputStore.ts'
import { useReplStore } from '../replStore.ts'

/** Host inputs for the widget. */
export interface ReplOutputDeps {
  /**
   * Resolve the kernel session id for a given REPL block. Returns
   * `null` when no session has been started yet for this cell —
   * the widget renders nothing in that case (no output, no pending
   * status).
   */
  resolveSessionId(block: ReplFenceBlock): string | null
}

class ReplOutputWidget extends WidgetType {
  private readonly sessionId: string
  private unsub: (() => void) | null = null
  private dom: HTMLElement | null = null

  constructor(sessionId: string) {
    super()
    this.sessionId = sessionId
  }

  // Equal widgets short-circuit CM6's diff so we don't remount on
  // every doc edit; same sessionId = same widget.
  eq(other: ReplOutputWidget): boolean {
    return other.sessionId === this.sessionId
  }

  toDOM(_view: EditorView): HTMLElement {
    const el = document.createElement('div')
    el.className = 'nexus-repl-output'
    el.dataset.sessionId = this.sessionId
    this.dom = el
    this.render()
    this.unsub = useReplOutputStore.subscribe(() => this.render())
    return el
  }

  /** Re-render from the store's current state. Only touches DOM
   *  text — no React, no full remount, cheap enough for high-
   *  frequency streaming output. */
  private render() {
    if (!this.dom) return
    const buf = useReplOutputStore.getState().buffers[this.sessionId]
    if (!buf || (buf.text.length === 0 && buf.startedAt === null)) {
      this.dom.textContent = ''
      this.dom.style.display = 'none'
      return
    }
    this.dom.style.display = ''
    this.dom.textContent = buf.text
  }

  destroy(_dom: HTMLElement): void {
    this.unsub?.()
    this.unsub = null
    this.dom = null
  }

  // Let clicks / selection pass through to the editor as if the
  // widget were ordinary text.
  ignoreEvent(_event: Event): boolean {
    return false
  }
}

/**
 * Effect used to nudge the StateField to rebuild when the
 * underlying session map changes (e.g. `ensureSession` just minted
 * a sessionId for a cell that previously resolved to `null`). The
 * payload is unused — the field re-runs `buildDecorations` against
 * the current state regardless.
 */
const refreshReplOutputDecorations = StateEffect.define<void>()

/**
 * Build a decoration set from a plain `EditorState`. One widget per
 * REPL block, placed just below the closing fence line. Caller-
 * supplied `resolveSessionId` returns `null` for cells that haven't
 * been eval'd yet — those get no widget.
 */
function buildDecorations(
  state: EditorState,
  deps: ReplOutputDeps,
): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>()
  const blocks = findReplBlocks(state.doc.toString())
  for (const block of blocks) {
    const sessionId = deps.resolveSessionId(block)
    if (!sessionId) continue
    // Anchor the widget at the END of the closing fence line so
    // CM6 renders it on the next line down — visually "below the
    // cell". `Decoration.widget` with `block: true` reserves a
    // full block-level slot.
    const lineCount = state.doc.lines
    const lineNo = Math.min(block.closeLine, lineCount)
    const lineEnd = state.doc.line(lineNo).to
    builder.add(
      lineEnd,
      lineEnd,
      Decoration.widget({
        widget: new ReplOutputWidget(sessionId),
        block: true,
        side: 1,
      }),
    )
  }
  return builder.finish()
}

/**
 * Root extension for the REPL output widget set. The StateField is
 * the source of truth for the decoration set (block decorations
 * must come from a field, not a plugin). A companion ViewPlugin
 * subscribes to `useReplStore` so the field rebuilds when a new
 * session is minted by `ensureSession` even though the doc itself
 * hasn't changed.
 */
export function replOutputExt(deps: ReplOutputDeps): Extension {
  const field = StateField.define<DecorationSet>({
    create: (state) => buildDecorations(state, deps),
    update(value, tr) {
      const forced = tr.effects.some((e) =>
        e.is(refreshReplOutputDecorations),
      )
      if (!tr.docChanged && !forced) return value
      return buildDecorations(tr.state, deps)
    },
    provide: (f) => EditorView.decorations.from(f),
  })

  // Subscribe to the session store so the field can refresh when
  // `ensureSession` lands a fresh sessionId. Dispatching from a
  // store-subscription callback is safe — we're outside any
  // in-progress CM6 update.
  const sessionWatcher = ViewPlugin.fromClass(
    class {
      private readonly view: EditorView
      private unsub: (() => void) | null = null
      private destroyed = false

      constructor(view: EditorView) {
        this.view = view
        this.unsub = useReplStore.subscribe(() => {
          if (this.destroyed) return
          this.view.dispatch({
            effects: refreshReplOutputDecorations.of(),
          })
        })
      }

      update(_u: ViewUpdate) {
        // No doc-change-driven dispatch — the StateField's `update`
        // hook already sees `tr.docChanged` directly.
      }

      destroy() {
        this.destroyed = true
        this.unsub?.()
        this.unsub = null
      }
    },
  )

  return [field, sessionWatcher]
}

export { refreshReplOutputDecorations }
