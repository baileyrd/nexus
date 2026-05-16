// BL-142 Phase 2a — per-tab REPL session bookkeeping.
//
// One REPL session per `(relpath, lang)` pair. The first
// `Shift-Enter` in a `python repl` block on `notes/scratch.md`
// spawns a `python` kernel for that tab; subsequent Shift-Enters
// reuse it. Closing the tab tears down every kernel tagged to
// `relpath`. Reopening the tab spawns fresh kernels — the Phase 2b
// editor REPL plugin will invoke `stopForTab(relpath)` from its
// tab-close lifecycle hook.
//
// State is intentionally view-only — REPL output flows on the
// existing `com.nexus.terminal.output.<sessionId>` bus topic, not
// through this store. Phase 2b's `<ReplOutput />` widget will read
// the sessionId from this store and subscribe to the bus directly.
//
// The store doesn't hold a `ReplClient` reference; every mutating
// action takes `client` as a parameter. Keeps the store pure +
// testable without standing up a Tauri runtime.

import { create } from 'zustand'

import { resolveKernelForLang } from './replKernels.ts'
import type { ReplClient } from './replClient.ts'

export type ReplSessionStatus = 'starting' | 'ready' | 'error'

export interface ReplSessionEntry {
  relpath: string
  lang: string
  /** Kernel session id returned by `repl_start`. `null` while
   *  `status === 'starting'` and on `'error'`. */
  sessionId: string | null
  status: ReplSessionStatus
  /** Error message when `status === 'error'`. */
  error?: string
  /** Unix epoch ms at start; `null` while starting. */
  startedAt: number | null
}

/** Map key — one entry per `(relpath, lang)`. */
function entryKey(relpath: string, lang: string): string {
  return `${relpath}::${lang}`
}

interface ReplState {
  /** Active + in-flight + errored sessions, keyed by `(relpath, lang)`. */
  sessions: Record<string, ReplSessionEntry>

  /**
   * Ensure a REPL session for `(relpath, lang)` exists, spawning
   * one via `client.start(...)` if missing. Returns the resolved
   * session id, or `null` if the user hasn't configured a kernel
   * for `lang` in `nexus.editor.replKernels` (the caller surfaces
   * a "configure a REPL kernel for X" message).
   *
   * Concurrency: if a second `ensureSession` lands while the first
   * is in flight, the second call observes `status === 'starting'`
   * and awaits the in-flight start by polling the store at
   * 25 ms — sufficient for the typing-hot path (Shift-Enter is
   * user-driven, not bursty). Phase 2b may replace with a proper
   * promise cache if real contention surfaces.
   */
  ensureSession(
    client: ReplClient,
    kernelsJson: string,
    relpath: string,
    lang: string,
  ): Promise<string | null>

  /**
   * Resolve a session id, run `client.eval(...)`. If no session
   * exists yet, spawns one via `ensureSession`. Returns `false`
   * when no kernel is configured for `lang`; the caller surfaces
   * the "configure a REPL kernel" message.
   */
  evalCode(
    client: ReplClient,
    kernelsJson: string,
    relpath: string,
    lang: string,
    code: string,
  ): Promise<boolean>

  /**
   * Close every REPL session tagged to `relpath` and drop the
   * store entries. Best-effort: a transport error on the
   * underlying `repl_stop` is swallowed, the store entry is
   * cleared regardless so the tab-reopen path gets a clean slate.
   */
  stopForTab(client: ReplClient, relpath: string): Promise<void>

  /** Close every active REPL session — plugin deactivation path. */
  stopAll(client: ReplClient): Promise<void>
}

const POLL_INTERVAL_MS = 25

export const useReplStore = create<ReplState>((set, get) => ({
  sessions: {},

  async ensureSession(client, kernelsJson, relpath, lang) {
    const key = entryKey(relpath, lang)
    const existing = get().sessions[key]
    if (existing && existing.status === 'ready' && existing.sessionId) {
      return existing.sessionId
    }
    if (existing && existing.status === 'starting') {
      // Another caller is in the middle of starting this same
      // (relpath, lang). Poll until it lands; fall through to the
      // spawn path if it ends up in 'error'.
      const deadline = Date.now() + 5000
      while (Date.now() < deadline) {
        await new Promise<void>((r) => setTimeout(r, POLL_INTERVAL_MS))
        const cur = get().sessions[key]
        if (!cur) break
        if (cur.status === 'ready' && cur.sessionId) return cur.sessionId
        if (cur.status === 'error') break
      }
    }

    const cmd = resolveKernelForLang(kernelsJson, lang)
    if (!cmd) return null

    set((s) => ({
      sessions: {
        ...s.sessions,
        [key]: {
          relpath,
          lang,
          sessionId: null,
          status: 'starting',
          startedAt: null,
        },
      },
    }))

    try {
      const resp = await client.start({
        lang,
        program: cmd.program,
        args: cmd.args,
      })
      set((s) => ({
        sessions: {
          ...s.sessions,
          [key]: {
            relpath,
            lang,
            sessionId: resp.id,
            status: 'ready',
            startedAt: Date.now(),
          },
        },
      }))
      return resp.id
    } catch (e) {
      set((s) => ({
        sessions: {
          ...s.sessions,
          [key]: {
            relpath,
            lang,
            sessionId: null,
            status: 'error',
            error: e instanceof Error ? e.message : String(e),
            startedAt: null,
          },
        },
      }))
      return null
    }
  },

  async evalCode(client, kernelsJson, relpath, lang, code) {
    const id = await get().ensureSession(client, kernelsJson, relpath, lang)
    if (id === null) return false
    await client.eval(id, code)
    return true
  },

  async stopForTab(client, relpath) {
    const entries = Object.entries(get().sessions).filter(
      ([, e]) => e.relpath === relpath,
    )
    for (const [key, entry] of entries) {
      if (entry.sessionId) {
        try {
          await client.stop(entry.sessionId)
        } catch {
          // Best-effort: the backend `repl_stop` may have already
          // GC'd if the kernel exited on its own (e.g. `:q` in a
          // python REPL). Either way, drop the store entry.
        }
      }
      set((s) => {
        const next = { ...s.sessions }
        delete next[key]
        return { sessions: next }
      })
    }
  },

  async stopAll(client) {
    const entries = Object.entries(get().sessions)
    for (const [key, entry] of entries) {
      if (entry.sessionId) {
        try {
          await client.stop(entry.sessionId)
        } catch {
          // ditto stopForTab
        }
      }
      set((s) => {
        const next = { ...s.sessions }
        delete next[key]
        return { sessions: next }
      })
    }
  },
}))

/** Test-only: reset the store back to its initial empty state. */
export function _resetReplStoreForTests(): void {
  useReplStore.setState({ sessions: {} })
}
