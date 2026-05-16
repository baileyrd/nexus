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

import { RangeSetBuilder, type Extension } from '@codemirror/state'
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
 * Build a decoration set for `view`. One widget per REPL block,
 * placed just below the closing fence line. Caller-supplied
 * `resolveSessionId` returns `null` for cells that haven't been
 * eval'd yet — those get no widget.
 */
function buildDecorations(
  view: EditorView,
  deps: ReplOutputDeps,
): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>()
  const blocks = findReplBlocks(view.state.doc.toString())
  for (const block of blocks) {
    const sessionId = deps.resolveSessionId(block)
    if (!sessionId) continue
    // Anchor the widget at the END of the closing fence line so
    // CM6 renders it on the next line down — visually "below the
    // cell". `Decoration.widget` with `block: true` reserves a
    // full block-level slot.
    const lineCount = view.state.doc.lines
    const lineNo = Math.min(block.closeLine, lineCount)
    const lineEnd = view.state.doc.line(lineNo).to
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
 * Root extension for the REPL output widget set. Re-derives the
 * decoration list whenever the doc changes (a new REPL fence
 * landing or being deleted shifts widget positions). The widget
 * subscribing to `useReplOutputStore` from its own DOM keeps
 * streaming output cheap — that path doesn't go through this
 * extension's decoration-rebuild loop.
 */
export function replOutputExt(deps: ReplOutputDeps): Extension {
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet

      constructor(view: EditorView) {
        this.decorations = buildDecorations(view, deps)
      }

      update(u: ViewUpdate) {
        if (u.docChanged) {
          this.decorations = buildDecorations(u.view, deps)
        }
      }
    },
    { decorations: (v) => v.decorations },
  )
}
