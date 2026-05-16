// BL-142 Phase 2b.2 — per-REPL-cell output buffer.
//
// Output streams asynchronously on the `com.nexus.terminal.output.<id>`
// kernel bus topic. The widget below each REPL cell needs cell-
// scoped state (not session-scoped — the cell is the user's
// concept). Map sessionId → output buffer; each buffer is the
// post-ANSI-strip rendered text + a "has any chunk arrived" flag
// so the widget can collapse to "(no output yet)".
//
// Keyed by sessionId rather than (relpath, lang) because the bus
// event carries the sessionId verbatim — the pump that routes bus
// events to this store doesn't have to reverse-resolve into the
// REPL session store on every chunk.

import { create } from 'zustand'

export interface ReplOutputBuffer {
  /** ANSI-stripped accumulated text. */
  text: string
  /** Wall-clock at which the current eval started (cleared on
   *  next `clear` call). Renderers can show "running for 1.2s"
   *  while output is still flowing. */
  startedAt: number | null
}

interface ReplOutputState {
  /** sessionId → buffer. Same key the kernel bus event carries
   *  in `com.nexus.terminal.output.<sessionId>`. */
  buffers: Record<string, ReplOutputBuffer>

  /** Reset the buffer for `sessionId` — called when the user
   *  fires a fresh eval so the widget shows just the new output. */
  clear(sessionId: string): void

  /** Append `chunk` (already ANSI-stripped by the pump) to the
   *  buffer for `sessionId`. Creates the buffer on first chunk
   *  if absent. */
  append(sessionId: string, chunk: string): void

  /** Drop the buffer entirely — called when `repl_stop` runs
   *  so we don't grow unboundedly across the plugin's lifetime. */
  drop(sessionId: string): void
}

export const useReplOutputStore = create<ReplOutputState>((set) => ({
  buffers: {},

  clear(sessionId) {
    set((s) => ({
      buffers: {
        ...s.buffers,
        [sessionId]: { text: '', startedAt: Date.now() },
      },
    }))
  },

  append(sessionId, chunk) {
    set((s) => {
      const cur = s.buffers[sessionId] ?? { text: '', startedAt: null }
      return {
        buffers: {
          ...s.buffers,
          [sessionId]: {
            text: cur.text + chunk,
            startedAt: cur.startedAt,
          },
        },
      }
    })
  },

  drop(sessionId) {
    set((s) => {
      const next = { ...s.buffers }
      delete next[sessionId]
      return { buffers: next }
    })
  },
}))

/** Test-only — reset the store back to its initial empty state. */
export function _resetReplOutputStoreForTests(): void {
  useReplOutputStore.setState({ buffers: {} })
}
