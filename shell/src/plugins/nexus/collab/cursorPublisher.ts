// BL-143 Phase 2.2 — CM6 cursor publisher.
//
// Observes selection changes in a CodeMirror view, debounces them,
// and invokes `com.nexus.collab/publish_presence` with the current
// caret offset (and `selection_end` when a range is selected). The
// handler stamps the local peer identity from `[collab]` and
// publishes on `com.nexus.collab.presence`; the Phase 1.5 bridge
// forwards to peers.
//
// Two pieces:
//
// 1. `createCursorPublisherCore` — pure debounce / disable / dedup
//    state machine. Doesn't import `@codemirror/*`, so unit tests can
//    drive it without a DOM.
// 2. `cursorPublisherExt` — CM6 `ViewPlugin` that observes
//    `update.selectionSet` and feeds the core.
//
// Disable behaviour: when no `[collab]` block is present the IPC
// handler returns a successful no-op reply `{ published: false }`. The
// core reads that flag and stops calling so a misconfigured forge isn't
// hammered by every keystroke. (A legacy `"collab not configured"`
// ExecutionFailed error is still honoured as a fallback.) The flag is
// process-lifetime; the user must reload after editing
// `.forge/config.toml`.

import { ViewPlugin, type ViewUpdate, type EditorView } from '@codemirror/view'
import type { Extension } from '@codemirror/state'

const COLLAB_PLUGIN_ID = 'com.nexus.collab'
const PUBLISH_PRESENCE = 'publish_presence'

/** Substring of the `ExecutionFailed.reason` returned by the handler
 *  when `[collab].peer_id` / `[collab].display_name` are empty in the
 *  forge config. The shell uses it as a stable signal to stop calling. */
const NOT_CONFIGURED_HINT = 'collab not configured'

const DEFAULT_DEBOUNCE_MS = 150

export interface CursorPublisherConfig {
  /** Forge-relative path of the file the user is editing. */
  relpath: string
  /** Kernel IPC bridge. Same signature as `api.kernel.invoke`. */
  invoke: (
    pluginId: string,
    commandId: string,
    args: unknown,
  ) => Promise<unknown>
  /** Debounce window. Defaults to 150ms (CD-meets-DoD's 200ms budget). */
  debounceMs?: number
  /** Optional clock override for tests. */
  now?: () => number
  /** Schedule a callback after `ms` ms. Defaults to `setTimeout`; tests
   *  pass a manual scheduler to step time. Returns a token the caller
   *  later passes to `clear`. */
  schedule?: (cb: () => void, ms: number) => unknown
  /** Cancel a scheduled callback. Defaults to `clearTimeout`. */
  clear?: (token: unknown) => void
}

export interface CursorSnapshot {
  /** Current caret position. */
  offset: number
  /** Other end of the selection range when not a caret. `undefined`
   *  means it's a plain caret position. */
  selectionEnd?: number
}

export interface CursorPublisherCore {
  /** Feed a fresh selection observation. The core debounces and
   *  invokes the IPC at most once per debounce window. */
  observe(snap: CursorSnapshot): void
  /** Cancel any pending invocation; idempotent. Called when the host
   *  view tears down. */
  destroy(): void
  /** True once the handler has surfaced "not configured"; the core
   *  has stopped calling. Visible for tests. */
  isDisabled(): boolean
}

/**
 * Pure debounce / dedup / disable-on-config-error state machine.
 *
 * Skips untitled buffers (`relpath` empty or `untitled:`-prefixed)
 * since the relay has no meaning for them. Drops repeat calls with
 * the same offset/selection — keystroke storms that don't move the
 * caret (e.g. typing in the middle of a word with `selectionSet`
 * firing because the doc changed) don't generate redundant traffic.
 */
export function createCursorPublisherCore(
  cfg: CursorPublisherConfig,
): CursorPublisherCore {
  const debounceMs = cfg.debounceMs ?? DEFAULT_DEBOUNCE_MS
  const schedule = cfg.schedule ?? ((cb, ms) => setTimeout(cb, ms))
  const clear = cfg.clear ?? ((token) => clearTimeout(token as ReturnType<typeof setTimeout>))
  const skipFile = cfg.relpath === '' || cfg.relpath.startsWith('untitled:')

  let disabled = skipFile
  let pending: unknown = null
  let lastSent: CursorSnapshot | null = null
  let nextSnap: CursorSnapshot | null = null

  function flush(): void {
    pending = null
    const snap = nextSnap
    nextSnap = null
    if (!snap || disabled) return
    if (
      lastSent &&
      lastSent.offset === snap.offset &&
      lastSent.selectionEnd === snap.selectionEnd
    ) {
      return
    }
    lastSent = snap
    const cursor: Record<string, unknown> = {
      relpath: cfg.relpath,
      offset: snap.offset,
    }
    if (snap.selectionEnd !== undefined && snap.selectionEnd !== snap.offset) {
      cursor.selection_end = snap.selectionEnd
    }
    cfg.invoke(COLLAB_PLUGIN_ID, PUBLISH_PRESENCE, { cursor })
      .then((reply) => {
        // Successful no-op: collab isn't configured, so the handler
        // returns `{ published: false }`. Stop pinging so a
        // misconfigured forge isn't hammered on every cursor move.
        const r = reply as { published?: boolean } | null | undefined
        if (r && r.published === false) {
          disabled = true
        }
      })
      .catch((err) => {
        // Legacy / defensive: older backends signalled "not configured"
        // via an ExecutionFailed error rather than `published: false`.
        const msg = String(err ?? '')
        if (msg.includes(NOT_CONFIGURED_HINT)) {
          disabled = true
        }
        // Other errors are silently swallowed — collab is opt-in and
        // a per-keystroke error toast would be noisy.
      })
  }

  return {
    observe(snap) {
      if (disabled) return
      nextSnap = snap
      if (pending !== null) clear(pending)
      pending = schedule(flush, debounceMs)
    },
    destroy() {
      if (pending !== null) {
        clear(pending)
        pending = null
      }
      nextSnap = null
    },
    isDisabled() {
      return disabled
    },
  }
}

/**
 * CM6 wrapper around [`createCursorPublisherCore`]. Adds a
 * `update.selectionSet` listener and feeds the core with the head /
 * anchor of the main selection.
 */
export function cursorPublisherExt(cfg: CursorPublisherConfig): Extension {
  return ViewPlugin.fromClass(
    class {
      private core: CursorPublisherCore
      constructor(view: EditorView) {
        this.core = createCursorPublisherCore(cfg)
        // Publish the initial caret position so peers see "X is on
        // file Y" before the user moves the cursor.
        const sel = view.state.selection.main
        this.core.observe({
          offset: sel.head,
          selectionEnd: sel.anchor !== sel.head ? sel.anchor : undefined,
        })
      }
      update(update: ViewUpdate): void {
        if (!update.selectionSet) return
        const sel = update.state.selection.main
        this.core.observe({
          offset: sel.head,
          selectionEnd: sel.anchor !== sel.head ? sel.anchor : undefined,
        })
      }
      destroy(): void {
        this.core.destroy()
      }
    },
  )
}
