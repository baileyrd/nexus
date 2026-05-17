// BL-143 Phase 2.2 — CM6 remote-cursor decoration layer.
//
// Subscribes to `useCollabStore` and renders a coloured caret + a
// hover tooltip carrying the display name for each peer whose
// `cursor.relpath` matches this editor's `relpath`. Peers focused on
// other files (or with no cursor) don't render. Removal happens
// automatically when the store evicts a peer.
//
// Pure pieces:
//
// - `buildRemoteCursorRanges(relpath, peers, docLength)` returns the
//   sorted (offset, range?, user_id, display_name, color) tuples this
//   editor should paint. Doesn't touch CM6, so the unit tests don't
//   need a DOM.
// - `colorForUserId(id)` is a stable hash → hue mapping. Same `id`
//   always gets the same colour across editors and across reloads.
//
// CM6 wiring:
//
// - `remoteCursorsExt({ relpath })` returns a `ViewPlugin` that
//   re-runs `buildRemoteCursorRanges` whenever the collab store
//   changes (subscribed once per view) and dispatches a state-field
//   effect carrying the new decoration set.

import {
  StateEffect,
  StateField,
  RangeSetBuilder,
  type Extension,
} from '@codemirror/state'
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  WidgetType,
  type ViewUpdate,
} from '@codemirror/view'
import { useCollabStore, type CollabPeer } from './collabStore'

// ── Pure data layer ───────────────────────────────────────────────────────────

export interface RemoteCursorRange {
  user_id: string
  display_name: string
  /** Caret position (CM offset). Always within `[0, docLength]`. */
  offset: number
  /** Selection-other-end when the peer has a non-collapsed range.
   *  `undefined` for caret-only positions. */
  selection_end?: number
  /** Stable hex color derived from `user_id`. */
  color: string
}

/**
 * 32-bit FNV-1a hash. Stable across browsers and Node (no `subtle`,
 * no `crypto` dependency), good enough for "pick a hue for this peer".
 */
function fnv1a(s: string): number {
  let h = 0x811c9dc5
  for (let i = 0; i < s.length; i += 1) {
    h ^= s.charCodeAt(i)
    h = Math.imul(h, 0x01000193)
  }
  return h >>> 0
}

/**
 * Map a user id to a stable HSL hex. 8 quantised hues × 1 saturation
 * × 1 lightness gives a palette readable on both light and dark
 * backgrounds without per-theme branching.
 */
export function colorForUserId(userId: string): string {
  const hue = Math.floor((fnv1a(userId) % 8) * 45)
  // HSL → RGB inline to avoid a runtime dependency. s=65%, l=55%.
  const s = 0.65
  const l = 0.55
  const c = (1 - Math.abs(2 * l - 1)) * s
  const x = c * (1 - Math.abs(((hue / 60) % 2) - 1))
  const m = l - c / 2
  let r = 0
  let g = 0
  let b = 0
  if (hue < 60) { r = c; g = x; b = 0 }
  else if (hue < 120) { r = x; g = c; b = 0 }
  else if (hue < 180) { r = 0; g = c; b = x }
  else if (hue < 240) { r = 0; g = x; b = c }
  else if (hue < 300) { r = x; g = 0; b = c }
  else { r = c; g = 0; b = x }
  const toHex = (v: number) => Math.round((v + m) * 255).toString(16).padStart(2, '0')
  return `#${toHex(r)}${toHex(g)}${toHex(b)}`
}

/**
 * Project the peer map onto this editor's relpath. Returns ranges
 * sorted by offset so the decoration builder can add them in order
 * (RangeSet requires sorted input).
 *
 * Out-of-range offsets are clamped to `[0, docLength]` so a stale
 * presence frame from before a big edit doesn't crash CM6. The peer
 * keeps rendering at the clamped position until they move.
 */
export function buildRemoteCursorRanges(
  relpath: string,
  peers: Record<string, CollabPeer>,
  docLength: number,
): RemoteCursorRange[] {
  const out: RemoteCursorRange[] = []
  for (const peer of Object.values(peers)) {
    const cur = peer.cursor
    if (!cur || cur.relpath !== relpath) continue
    if (cur.offset === undefined) continue
    const offset = Math.min(Math.max(0, cur.offset), docLength)
    const range: RemoteCursorRange = {
      user_id: peer.user_id,
      display_name: peer.display_name,
      offset,
      color: colorForUserId(peer.user_id),
    }
    if (cur.selection_end !== undefined && cur.selection_end !== cur.offset) {
      range.selection_end = Math.min(Math.max(0, cur.selection_end), docLength)
    }
    out.push(range)
  }
  out.sort((a, b) => a.offset - b.offset || a.user_id.localeCompare(b.user_id))
  return out
}

// ── CM6 wiring ────────────────────────────────────────────────────────────────

class RemoteCaretWidget extends WidgetType {
  constructor(private readonly range: RemoteCursorRange) {
    super()
  }
  toDOM(): HTMLElement {
    const el = document.createElement('span')
    el.className = 'nexus-collab-remote-caret'
    el.style.borderLeft = `2px solid ${this.range.color}`
    el.style.marginLeft = '-1px'
    el.style.height = '1em'
    el.style.display = 'inline-block'
    el.style.position = 'relative'
    el.title = this.range.display_name
    // Tiny dot above the caret carrying the peer's color so the
    // user can spot the cursor without hovering.
    const dot = document.createElement('span')
    dot.style.position = 'absolute'
    dot.style.top = '-4px'
    dot.style.left = '-3px'
    dot.style.width = '6px'
    dot.style.height = '6px'
    dot.style.borderRadius = '3px'
    dot.style.background = this.range.color
    el.appendChild(dot)
    return el
  }
  eq(other: WidgetType): boolean {
    return (
      other instanceof RemoteCaretWidget &&
      other.range.user_id === this.range.user_id &&
      other.range.offset === this.range.offset &&
      other.range.color === this.range.color
    )
  }
  ignoreEvent(): boolean {
    return false
  }
}

const setRemoteCursors = StateEffect.define<RemoteCursorRange[]>()

const remoteCursorsField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, tr) {
    for (const e of tr.effects) {
      if (!e.is(setRemoteCursors)) continue
      const builder = new RangeSetBuilder<Decoration>()
      const docLen = tr.state.doc.length
      // Re-clamp in the field too — the doc could have shrunk
      // between the store dispatch and this transaction.
      const sorted = e.value
        .map((r) => ({
          ...r,
          offset: Math.min(r.offset, docLen),
          selection_end:
            r.selection_end !== undefined
              ? Math.min(r.selection_end, docLen)
              : undefined,
        }))
        .sort((a, b) => a.offset - b.offset)
      for (const r of sorted) {
        if (r.selection_end !== undefined && r.selection_end !== r.offset) {
          const [a, b] = r.offset < r.selection_end
            ? [r.offset, r.selection_end]
            : [r.selection_end, r.offset]
          builder.add(
            a,
            b,
            Decoration.mark({
              attributes: {
                style: `background-color: ${r.color}33;`,
                title: r.display_name,
              },
              class: 'nexus-collab-remote-selection',
            }),
          )
        }
        builder.add(
          r.offset,
          r.offset,
          Decoration.widget({
            widget: new RemoteCaretWidget(r),
            side: 1,
          }),
        )
      }
      return builder.finish()
    }
    return value.map(tr.changes)
  },
  provide: (f) => EditorView.decorations.from(f),
})

export interface RemoteCursorsConfig {
  /** Forge-relative path of the file this editor renders. */
  relpath: string
}

/**
 * CM6 extension: subscribes to the collab store and re-decorates the
 * view whenever the peer map changes. The subscription is torn down
 * in `destroy()` so a tab close doesn't leak. Untitled buffers are a
 * no-op (no remote peer can target them — `relpath` never matches).
 */
export function remoteCursorsExt(cfg: RemoteCursorsConfig): Extension {
  return [
    remoteCursorsField,
    ViewPlugin.fromClass(
      class {
        private unsubscribe: () => void
        private destroyed = false
        constructor(view: EditorView) {
          // Defer dispatches so we don't call `view.dispatch` while
          // CM6 is still inside an update — both the initial apply
          // (constructor runs during the view's first update) and any
          // store notification that lands mid-transaction would
          // otherwise throw "Calls to EditorView.update are not
          // allowed while an update is in progress".
          const apply = (peers: Record<string, CollabPeer>) => {
            queueMicrotask(() => {
              if (this.destroyed) return
              const ranges = buildRemoteCursorRanges(
                cfg.relpath,
                peers,
                view.state.doc.length,
              )
              view.dispatch({ effects: setRemoteCursors.of(ranges) })
            })
          }
          this.unsubscribe = useCollabStore.subscribe((s, prev) => {
            if (s.peers !== prev.peers) apply(s.peers)
          })
          apply(useCollabStore.getState().peers)
        }
        update(_update: ViewUpdate): void {
          // Decorations themselves track the doc through `value.map`
          // in the field's update; no per-keystroke re-fetch needed.
        }
        destroy(): void {
          this.destroyed = true
          this.unsubscribe()
        }
      },
    ),
  ]
}
