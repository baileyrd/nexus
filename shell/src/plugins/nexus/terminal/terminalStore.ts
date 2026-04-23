import { create } from 'zustand'

/**
 * Shell-side view-model for `nexus.terminal`.
 *
 * Holds the current session id (assigned when the kernel's
 * `com.nexus.terminal::create_session` returns) and a coarse
 * visibility flag mirrored from layoutStore. `visible` is redundant
 * with `layoutStore.panelArea.visible` but kept here so TerminalView
 * can read a single source without subscribing to the whole layout
 * store.
 *
 * WI-12 (TS half) — also owns the per-session stream-bookkeeping for
 * the `com.nexus.terminal.output.<session_id>` kernel event topic. The
 * subscription is wired up in `index.ts::activate`; bytes arrive via
 * `handleStreamChunk` and are routed to the registered xterm sink for
 * that session. Multiple sessions never cross-contaminate because the
 * sink registry is keyed by session id; the current single-session UI
 * still benefits because a stale session id (workspace-switch race)
 * has no sink and its chunks are simply dropped.
 */

/**
 * Payload shape from `OutputStreamPayload` (see
 * crates/nexus-terminal/src/core_plugin.rs). Tauri serialises Vec<u8>
 * as a JSON `number[]`, not base64; the TS side normalises to
 * `Uint8Array` at the boundary so xterm.js `write` can consume it
 * directly.
 */
export interface OutputStreamPayload {
  data: number[]
  seq: number
  ts_ms: number
}

/** Function the per-session xterm registers; receives raw PTY bytes. */
export type SessionSink = (bytes: Uint8Array) => void

/**
 * Lag-recovery callback supplied by `index.ts::activate`. The store
 * stays IPC-agnostic — it only knows "ask the kernel for bytes since
 * `cursor`" — so the call signature mirrors the kernel's
 * `read_raw_since` response. Returning `null` signals the recovery
 * call failed and the caller should reset to a clean slate.
 */
export type RecoverFn = (
  sessionId: string,
  lastCursor: number,
) => Promise<{ cursor: number; data: Uint8Array } | null>

interface SessionStreamState {
  /** Last chunk `seq` we accepted; 0 means no chunks yet. */
  lastSeq: number
  /**
   * Monotonic byte offset of the last byte handed to the sink. Same
   * coordinate system as `read_raw_since`'s cursor — both are the PTY
   * ring's absolute position — so it's safe to feed back into
   * `recoverFn` on a gap.
   */
  lastCursor: number
  /** True between gap detection and recovery completion. */
  recoveryInFlight: boolean
}

interface TerminalState {
  sessionId: string | null
  visible: boolean
  streams: Record<string, SessionStreamState>
  sinks: Record<string, SessionSink>
  recoverFn: RecoverFn | null

  setSession(id: string | null): void
  setVisible(v: boolean): void

  /**
   * Register the sink for `sessionId`. Overwrites any prior sink (a
   * remount of the xterm view installs a new one). Returns an
   * unregister fn that's a no-op if a different sink has since taken
   * over.
   */
  registerSink(sessionId: string, sink: SessionSink): () => void

  /** Wire (or clear) the recovery callback. */
  setRecoverFn(fn: RecoverFn | null): void

  /**
   * Route an output chunk for `sessionId`. Detects seq gaps and
   * triggers recovery via `recoverFn` when one is observed.
   *
   * Out-of-order / gap behaviour: on detecting `seq !== lastSeq + 1`
   * we drop the offending chunk and call `recoverFn` with our last
   * cursor; the snapshot's bytes are written to the sink and
   * `lastCursor` advances. Per the WI-12 brief option (a) we then
   * accept the *next* chunk's seq as the new baseline (i.e. set
   * `lastSeq = 0` so the next-arriving chunk's seq becomes the new
   * baseline) — `read_raw_since` is byte-authoritative so continuity
   * post-recovery is guaranteed by bytes, not by seq.
   */
  handleStreamChunk(sessionId: string, payload: OutputStreamPayload): void

  /**
   * Synchronise `lastCursor` to a value the pump path observed (used
   * by the 5s defensive heartbeat in TerminalView). Only advances —
   * never rewinds — so a slow stream chunk arriving after a pump
   * can't undo the catch-up.
   */
  advanceCursor(sessionId: string, cursor: number): void

  /** Clear all per-session bookkeeping — used on workspace close. */
  resetStreams(): void
}

function emptyStream(): SessionStreamState {
  return { lastSeq: 0, lastCursor: 0, recoveryInFlight: false }
}

export const useTerminalStore = create<TerminalState>((set, get) => ({
  sessionId: null,
  visible: false,
  streams: {},
  sinks: {},
  recoverFn: null,

  setSession: (id) => set({ sessionId: id }),
  setVisible: (v) => set({ visible: v }),

  registerSink: (sessionId, sink) => {
    set((s) => ({ sinks: { ...s.sinks, [sessionId]: sink } }))
    return () => {
      const current = get().sinks[sessionId]
      if (current === sink) {
        set((s) => {
          const next = { ...s.sinks }
          delete next[sessionId]
          return { sinks: next }
        })
      }
    }
  },

  setRecoverFn: (fn) => set({ recoverFn: fn }),

  handleStreamChunk: (sessionId, payload) => {
    const state = get()
    const stream = state.streams[sessionId] ?? emptyStream()
    const sink = state.sinks[sessionId]

    // Recovery in flight — drop the chunk; the snapshot bytes will
    // cover the gap and any subsequent chunks beyond that re-baseline.
    if (stream.recoveryInFlight) return

    // Gap detection: lastSeq === 0 means we're at the baseline (first
    // chunk after subscription / session start / post-recovery), so
    // any seq is acceptable and becomes the new baseline.
    const isFirst = stream.lastSeq === 0
    const expected = stream.lastSeq + 1
    if (!isFirst && payload.seq !== expected) {
      // Trigger recovery via read_raw_since. This is fire-and-forget;
      // the store stays sync. Subsequent chunks arriving while
      // `recoveryInFlight` is true are dropped above.
      const recover = state.recoverFn
      if (recover) {
        const next: SessionStreamState = {
          ...stream,
          recoveryInFlight: true,
        }
        set((s) => ({ streams: { ...s.streams, [sessionId]: next } }))
        void recover(sessionId, stream.lastCursor).then((snapshot) => {
          const after = get()
          const liveSink = after.sinks[sessionId]
          if (snapshot && liveSink) {
            liveSink(snapshot.data)
          }
          // Option (a): post-recovery, accept the next chunk's seq as
          // the new baseline. lastSeq=0 signals "first chunk wins".
          set((s) => ({
            streams: {
              ...s.streams,
              [sessionId]: {
                lastSeq: 0,
                lastCursor: snapshot ? snapshot.cursor : stream.lastCursor,
                recoveryInFlight: false,
              },
            },
          }))
        })
      } else {
        // No recoverFn wired — best effort, accept the chunk and
        // re-baseline so we don't loop on every subsequent gap.
        const bytes = new Uint8Array(payload.data)
        if (sink) sink(bytes)
        set((s) => ({
          streams: {
            ...s.streams,
            [sessionId]: {
              lastSeq: payload.seq,
              lastCursor: stream.lastCursor + bytes.length,
              recoveryInFlight: false,
            },
          },
        }))
      }
      return
    }

    // Normal path: write bytes, advance cursor + seq.
    const bytes = new Uint8Array(payload.data)
    if (sink) sink(bytes)
    set((s) => ({
      streams: {
        ...s.streams,
        [sessionId]: {
          lastSeq: payload.seq,
          lastCursor: stream.lastCursor + bytes.length,
          recoveryInFlight: false,
        },
      },
    }))
  },

  advanceCursor: (sessionId, cursor) => {
    const stream = get().streams[sessionId] ?? emptyStream()
    if (cursor <= stream.lastCursor) return
    set((s) => ({
      streams: {
        ...s.streams,
        [sessionId]: { ...stream, lastCursor: cursor },
      },
    }))
  },

  resetStreams: () => set({ streams: {}, sinks: {} }),
}))
