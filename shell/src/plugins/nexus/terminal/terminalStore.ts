import { create } from 'zustand'

/**
 * Shell-side view-model for `nexus.terminal`.
 *
 * Holds the open terminal tabs (each backed by a kernel session created
 * via `com.nexus.terminal::create_session`), which one is active, and a
 * coarse visibility flag mirrored from layoutStore. `visible` is
 * redundant with `layoutStore.panelArea.visible` but kept here so the
 * terminal views can read a single source without subscribing to the
 * whole layout store.
 *
 * Multi-terminal tabs (Zed-style): the panel hosts one leaf, and that
 * leaf renders a tab strip plus one live xterm per tab. Each tab owns a
 * distinct kernel session id; switching tabs only flips which xterm is
 * visible, so every terminal keeps its own scrollback and PTY state.
 *
 * WI-12 (TS half) — also owns the per-session stream-bookkeeping for
 * the `com.nexus.terminal.output.<session_id>` kernel event topic. The
 * subscription is wired up in `index.ts::activate`; bytes arrive via
 * `handleStreamChunk` and are routed to the registered xterm sink for
 * that session. Multiple sessions never cross-contaminate because the
 * sink registry is keyed by session id.
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

/** One open terminal tab. `id` is the kernel session id. */
export interface TerminalTab {
  id: string
  title: string
  /**
   * True once the user has manually renamed the tab. A pinned title is
   * never overwritten by the auto-naming path (OSC title / cwd), so a
   * deliberate name survives later shell-driven title changes. Cleared
   * only by reverting to auto-naming (not currently exposed in the UI).
   */
  custom: boolean
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
  /** Open tabs, in display order (left → right). */
  tabs: TerminalTab[]
  /** Session id of the active (foreground) tab, or null when none. */
  activeSessionId: string | null
  visible: boolean
  streams: Record<string, SessionStreamState>
  sinks: Record<string, SessionSink>
  recoverFn: RecoverFn | null

  /**
   * Append a tab and make it active. `custom` defaults to `false` (an
   * auto-named tab) when omitted, so callers that don't care about
   * pinning can pass just `{ id, title }`.
   */
  addTab(tab: Omit<TerminalTab, 'custom'> & { custom?: boolean }): void
  /**
   * Remove a tab. If it was the active one, activate a neighbour
   * (prefer the tab to its left, else to its right, else null).
   */
  removeTab(id: string): void
  /** Set the active tab. `null` clears it (no terminals open). */
  setActiveSession(id: string | null): void
  /**
   * Manually rename a tab. Sets the title and pins it (`custom = true`)
   * so the auto-naming path leaves it alone afterwards. No-op for an
   * unknown id.
   */
  renameTab(id: string, title: string): void
  /**
   * Apply an auto-derived title (OSC window title or cwd). Ignored when
   * the tab is pinned by a manual rename, or when `title` is blank, or
   * when it matches the current title (avoids needless re-renders from a
   * shell that repaints its title on every prompt). No-op for an unknown
   * id.
   */
  applyAutoTitle(id: string, title: string): void
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
   * by the 5s defensive heartbeat in the terminal view). Only advances
   * — never rewinds — so a slow stream chunk arriving after a pump
   * can't undo the catch-up.
   */
  advanceCursor(sessionId: string, cursor: number): void

  /**
   * Clear all per-session bookkeeping (tabs, active id, streams,
   * sinks) — used on workspace close.
   */
  resetStreams(): void
}

function emptyStream(): SessionStreamState {
  return { lastSeq: 0, lastCursor: 0, recoveryInFlight: false }
}

export const useTerminalStore = create<TerminalState>((set, get) => ({
  tabs: [],
  activeSessionId: null,
  visible: false,
  streams: {},
  sinks: {},
  recoverFn: null,

  addTab: (tab) =>
    set((s) => {
      if (s.tabs.some((t) => t.id === tab.id)) {
        return { activeSessionId: tab.id }
      }
      const full: TerminalTab = { custom: false, ...tab }
      return { tabs: [...s.tabs, full], activeSessionId: tab.id }
    }),

  removeTab: (id) =>
    set((s) => {
      const idx = s.tabs.findIndex((t) => t.id === id)
      if (idx === -1) return {}
      const tabs = s.tabs.filter((t) => t.id !== id)
      let activeSessionId = s.activeSessionId
      if (s.activeSessionId === id) {
        // Prefer the left neighbour, then the right, then nothing.
        const neighbour = tabs[idx - 1] ?? tabs[idx] ?? null
        activeSessionId = neighbour ? neighbour.id : null
      }
      return { tabs, activeSessionId }
    }),

  setActiveSession: (id) => set({ activeSessionId: id }),

  renameTab: (id, title) =>
    set((s) => ({
      tabs: s.tabs.map((t) =>
        t.id === id ? { ...t, title, custom: true } : t,
      ),
    })),

  applyAutoTitle: (id, title) => {
    const trimmed = title.trim()
    if (trimmed.length === 0) return
    set((s) => {
      const tab = s.tabs.find((t) => t.id === id)
      if (!tab || tab.custom || tab.title === trimmed) return {}
      return {
        tabs: s.tabs.map((t) =>
          t.id === id ? { ...t, title: trimmed } : t,
        ),
      }
    })
  },

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

  resetStreams: () =>
    set({ streams: {}, sinks: {}, tabs: [], activeSessionId: null }),
}))
