import { create } from 'zustand'

/**
 * Shell-side view-model for `nexus.terminal`.
 *
 * Supports multiple concurrent PTY sessions — one per saved command that
 * the user has started, plus ad-hoc sessions opened via "New Terminal".
 * The active session drives what TerminalView renders; the saved-commands
 * sidebar shows a status dot per entry and lets the user switch sessions
 * by clicking a row.
 *
 * WI-12 (TS half) — also owns the per-session stream-bookkeeping for
 * the `com.nexus.terminal.output.<session_id>` kernel event topic. The
 * subscription is wired up in `index.ts::activate`; bytes arrive via
 * `handleStreamChunk` and are routed to the registered xterm sink for
 * that session.
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

/** Metadata tracked client-side for each open session. */
export interface SessionEntry {
  /** Human-readable label shown in the sidebar and the terminal header. */
  name: string
  /**
   * Slug of the saved command that spawned this session, if any.
   * Ad-hoc sessions (opened via "New Terminal") leave this undefined.
   */
  savedCommandSlug?: string
}

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
  /** Session currently rendered in the terminal pane. */
  activeSessionId: string | null
  /** All open sessions keyed by session id. */
  sessions: Record<string, SessionEntry>
  /**
   * Reverse lookup: saved-command slug → session id. Only populated for
   * sessions that were spawned from a saved command.
   */
  slugSessions: Record<string, string>
  /** Coarse panel-visibility flag mirrored from layoutStore. */
  visible: boolean

  streams: Record<string, SessionStreamState>
  sinks: Record<string, SessionSink>
  recoverFn: RecoverFn | null

  /** Register a new open session in the store. */
  addSession(id: string, entry: SessionEntry): void
  /** Remove a session (e.g. after close_session). Switches active to another session if needed. */
  removeSession(id: string): void
  /** Switch which session the terminal pane renders. */
  setActiveSession(id: string | null): void
  /** Backward-compat alias for setActiveSession; used by test reset helpers. */
  setSession(id: string | null): void
  /** Clear all session metadata (called on workspace close). */
  resetSessions(): void

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
   */
  handleStreamChunk(sessionId: string, payload: OutputStreamPayload): void

  /**
   * Synchronise `lastCursor` to a value the pump path observed (used
   * by the 5s defensive heartbeat in TerminalView). Only advances —
   * never rewinds.
   */
  advanceCursor(sessionId: string, cursor: number): void

  /** Clear all per-session stream bookkeeping and sinks. */
  resetStreams(): void
}

function emptyStream(): SessionStreamState {
  return { lastSeq: 0, lastCursor: 0, recoveryInFlight: false }
}

export const useTerminalStore = create<TerminalState>((set, get) => ({
  activeSessionId: null,
  sessions: {},
  slugSessions: {},
  visible: false,
  streams: {},
  sinks: {},
  recoverFn: null,

  addSession: (id, entry) =>
    set((s) => ({
      sessions: { ...s.sessions, [id]: entry },
      slugSessions: entry.savedCommandSlug
        ? { ...s.slugSessions, [entry.savedCommandSlug]: id }
        : s.slugSessions,
      activeSessionId: s.activeSessionId ?? id,
    })),

  removeSession: (id) =>
    set((s) => {
      const nextSessions = { ...s.sessions }
      delete nextSessions[id]

      const nextSlug = { ...s.slugSessions }
      for (const [slug, sid] of Object.entries(nextSlug)) {
        if (sid === id) delete nextSlug[slug]
      }

      // If the removed session was active, fall through to the first
      // remaining session (sidebar order) or null.
      const nextActive =
        s.activeSessionId === id
          ? (Object.keys(nextSessions)[0] ?? null)
          : s.activeSessionId

      return { sessions: nextSessions, slugSessions: nextSlug, activeSessionId: nextActive }
    }),

  setActiveSession: (id) => set({ activeSessionId: id }),

  setSession: (id) => {
    if (id === null) {
      set({ activeSessionId: null })
    } else {
      set({ activeSessionId: id })
    }
  },

  resetSessions: () => set({ sessions: {}, slugSessions: {}, activeSessionId: null }),

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

    if (stream.recoveryInFlight) return

    const isFirst = stream.lastSeq === 0
    const expected = stream.lastSeq + 1
    if (!isFirst && payload.seq !== expected) {
      const recover = state.recoverFn
      if (recover) {
        const next: SessionStreamState = { ...stream, recoveryInFlight: true }
        set((s) => ({ streams: { ...s.streams, [sessionId]: next } }))
        void recover(sessionId, stream.lastCursor).then((snapshot) => {
          const after = get()
          const liveSink = after.sinks[sessionId]
          if (snapshot && liveSink) {
            liveSink(snapshot.data)
          }
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
