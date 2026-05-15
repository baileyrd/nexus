// shell/src/plugins/nexus/debugger/debuggerStore.ts
//
// BL-081 — view model for the debugger panel.
//
// Single active session at a time (BL-081 first cut posture). Holds:
//   - per-source breakpoint sets (the source of truth for the gutter
//     once the editor wires it; today only the panel reads it)
//   - currently-known threads (refreshed on `stopped` event)
//   - top frame + frames + per-frame scopes + per-scope variables
//   - rolling output log from `output` events (capped)
//   - watch expressions (evaluated against the current top frame)
//
// The store does NOT subscribe to bus events directly — `index.ts`
// owns the subscription and dispatches actions. Keeps the store pure
// for unit testing.

import { create } from 'zustand'

import type {
  DapKernelAPI,
  DapScope,
  DapSourceBreakpoint,
  DapStackFrame,
  DapThread,
  DapVariable,
} from './debuggerIpc'
import {
  configurationDone,
  continueExecution,
  disconnect,
  evaluate,
  launch,
  next,
  pause,
  scopes,
  setBreakpoints,
  stackTrace,
  stepIn,
  stepOut,
  terminate,
  threads,
  variables,
  type LaunchOpts,
} from './debuggerIpc'

/** Snapshot of an output line for the bottom log pane. */
export interface OutputLine {
  category: string
  text: string
  ts: number
}

export interface WatchEntry {
  expression: string
  value: string | null
  error: string | null
}

interface DebuggerState {
  /** Currently-active adapter name. `null` until a session launches. */
  activeAdapter: string | null
  /** `true` between `launch` and `terminated`/`exited`. */
  running: boolean
  /** Reason for the current stop, if stopped. `null` while running. */
  stoppedReason: string | null
  /** Most recently stopped thread id. */
  currentThread: number | null
  /** Threads list — refreshed on `stopped`. */
  threads: DapThread[]
  /** Current top frame's id. Drives the Watch / Variables surface. */
  currentFrame: number | null
  /** Stack frames for `currentThread`. */
  frames: DapStackFrame[]
  /** Scopes for `currentFrame`. */
  scopes: DapScope[]
  /** Variables keyed by `variablesReference` (lazy-loaded on expand). */
  variablesByRef: Record<number, DapVariable[]>
  /** Per-source breakpoint sets. Keyed by absolute path. */
  breakpointsByPath: Record<string, DapSourceBreakpoint[]>
  /** Rolling output log. Capped at OUTPUT_CAP entries. */
  output: OutputLine[]
  /** Watch expressions. Evaluated on every `stopped` event. */
  watches: WatchEntry[]
  /** Last surfaced error (non-fatal). */
  error: string | null

  // ── actions ─────────────────────────────────────────────────────────────
  startSession(api: DapKernelAPI, opts: LaunchOpts): Promise<void>
  endSession(api: DapKernelAPI): Promise<void>
  terminateSession(api: DapKernelAPI): Promise<void>

  toggleBreakpoint(api: DapKernelAPI, path: string, line: number): Promise<void>
  clearBreakpointsForPath(api: DapKernelAPI, path: string): Promise<void>

  doContinue(api: DapKernelAPI): Promise<void>
  doNext(api: DapKernelAPI): Promise<void>
  doStepIn(api: DapKernelAPI): Promise<void>
  doStepOut(api: DapKernelAPI): Promise<void>
  doPause(api: DapKernelAPI): Promise<void>

  refreshAfterStop(
    api: DapKernelAPI,
    thread_id: number,
    reason: string,
  ): Promise<void>
  loadVariables(api: DapKernelAPI, ref: number): Promise<void>

  addWatch(expression: string): void
  removeWatch(expression: string): void
  reevaluateWatches(api: DapKernelAPI): Promise<void>

  pushOutput(category: string, text: string): void

  /** Mark the session as terminated. Idempotent. */
  markTerminated(): void
  /** Drop every field — used on workspace close. */
  reset(): void
}

const OUTPUT_CAP = 1000

const INITIAL = {
  activeAdapter: null as string | null,
  running: false,
  stoppedReason: null as string | null,
  currentThread: null as number | null,
  threads: [] as DapThread[],
  currentFrame: null as number | null,
  frames: [] as DapStackFrame[],
  scopes: [] as DapScope[],
  variablesByRef: {} as Record<number, DapVariable[]>,
  breakpointsByPath: {} as Record<string, DapSourceBreakpoint[]>,
  output: [] as OutputLine[],
  watches: [] as WatchEntry[],
  error: null as string | null,
}

export const useDebuggerStore = create<DebuggerState>((set, get) => ({
  ...INITIAL,

  async startSession(api, opts) {
    set({ activeAdapter: opts.adapter, error: null, output: [] })
    try {
      await launch(api, opts)
      // Re-issue every cached breakpoint set so the new session
      // honours them. configurationDone closes the handshake.
      const bps = get().breakpointsByPath
      for (const [path, lines] of Object.entries(bps)) {
        if (lines.length > 0) {
          await setBreakpoints(api, opts.adapter, path, lines)
        }
      }
      await configurationDone(api, opts.adapter)
      set({ running: true })
    } catch (e) {
      set({ activeAdapter: null, error: errMsg(e), running: false })
    }
  },

  async endSession(api) {
    const adapter = get().activeAdapter
    if (!adapter) return
    try {
      await disconnect(api, adapter, false)
    } catch (e) {
      set({ error: errMsg(e) })
    } finally {
      get().markTerminated()
    }
  },

  async terminateSession(api) {
    const adapter = get().activeAdapter
    if (!adapter) return
    try {
      await terminate(api, adapter)
    } catch (e) {
      set({ error: errMsg(e) })
    } finally {
      get().markTerminated()
    }
  },

  async toggleBreakpoint(api, path, line) {
    const cur = get().breakpointsByPath[path] ?? []
    const next = cur.some((b) => b.line === line)
      ? cur.filter((b) => b.line !== line)
      : [...cur, { line }]
    set({
      breakpointsByPath: { ...get().breakpointsByPath, [path]: next },
    })
    const adapter = get().activeAdapter
    if (!adapter) return
    try {
      await setBreakpoints(api, adapter, path, next)
    } catch (e) {
      set({ error: errMsg(e) })
    }
  },

  async clearBreakpointsForPath(api, path) {
    set({
      breakpointsByPath: { ...get().breakpointsByPath, [path]: [] },
    })
    const adapter = get().activeAdapter
    if (!adapter) return
    try {
      await setBreakpoints(api, adapter, path, [])
    } catch (e) {
      set({ error: errMsg(e) })
    }
  },

  async doContinue(api) {
    const adapter = get().activeAdapter
    const tid = get().currentThread
    if (!adapter || tid == null) return
    set({ stoppedReason: null, currentFrame: null, frames: [], scopes: [] })
    await continueExecution(api, adapter, tid).catch((e) =>
      set({ error: errMsg(e) }),
    )
  },

  async doNext(api) {
    const adapter = get().activeAdapter
    const tid = get().currentThread
    if (!adapter || tid == null) return
    await next(api, adapter, tid).catch((e) => set({ error: errMsg(e) }))
  },

  async doStepIn(api) {
    const adapter = get().activeAdapter
    const tid = get().currentThread
    if (!adapter || tid == null) return
    await stepIn(api, adapter, tid).catch((e) => set({ error: errMsg(e) }))
  },

  async doStepOut(api) {
    const adapter = get().activeAdapter
    const tid = get().currentThread
    if (!adapter || tid == null) return
    await stepOut(api, adapter, tid).catch((e) => set({ error: errMsg(e) }))
  },

  async doPause(api) {
    const adapter = get().activeAdapter
    const tid = get().currentThread ?? get().threads[0]?.id
    if (!adapter || tid == null) return
    await pause(api, adapter, tid).catch((e) => set({ error: errMsg(e) }))
  },

  async refreshAfterStop(api, thread_id, reason) {
    const adapter = get().activeAdapter
    if (!adapter) return
    set({
      currentThread: thread_id,
      stoppedReason: reason,
      error: null,
    })
    try {
      const t = await threads(api, adapter)
      set({ threads: t.threads })
      const trace = await stackTrace(api, adapter, thread_id)
      const frames = trace.stackFrames
      set({ frames })
      const top = frames[0]
      if (top) {
        set({ currentFrame: top.id })
        const sc = await scopes(api, adapter, top.id)
        set({ scopes: sc.scopes })
      } else {
        set({ currentFrame: null, scopes: [] })
      }
      await get().reevaluateWatches(api)
    } catch (e) {
      set({ error: errMsg(e) })
    }
  },

  async loadVariables(api, ref) {
    const adapter = get().activeAdapter
    if (!adapter) return
    try {
      const r = await variables(api, adapter, ref)
      set({
        variablesByRef: { ...get().variablesByRef, [ref]: r.variables },
      })
    } catch (e) {
      set({ error: errMsg(e) })
    }
  },

  addWatch(expression) {
    const trimmed = expression.trim()
    if (!trimmed) return
    if (get().watches.some((w) => w.expression === trimmed)) return
    set({
      watches: [
        ...get().watches,
        { expression: trimmed, value: null, error: null },
      ],
    })
  },

  removeWatch(expression) {
    set({ watches: get().watches.filter((w) => w.expression !== expression) })
  },

  async reevaluateWatches(api) {
    const adapter = get().activeAdapter
    const frame = get().currentFrame
    if (!adapter) return
    const current = get().watches
    const updated: WatchEntry[] = await Promise.all(
      current.map(async (w) => {
        try {
          const r = await evaluate(api, adapter, w.expression, frame ?? undefined, 'watch')
          return { expression: w.expression, value: r.result, error: null }
        } catch (e) {
          return { expression: w.expression, value: null, error: errMsg(e) }
        }
      }),
    )
    set({ watches: updated })
  },

  pushOutput(category, text) {
    const cur = get().output
    const next = [...cur, { category, text, ts: Date.now() }]
    // Trim from the front so the most recent OUTPUT_CAP lines win.
    if (next.length > OUTPUT_CAP) next.splice(0, next.length - OUTPUT_CAP)
    set({ output: next })
  },

  markTerminated() {
    set({
      activeAdapter: null,
      running: false,
      stoppedReason: null,
      currentThread: null,
      currentFrame: null,
      threads: [],
      frames: [],
      scopes: [],
      variablesByRef: {},
    })
  },

  reset() {
    set({ ...INITIAL })
  },
}))

function errMsg(e: unknown): string {
  if (e instanceof Error) return e.message
  return String(e)
}
