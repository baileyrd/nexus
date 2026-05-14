// BL-127 Phase A — production-side typing-latency instrumentation.
//
// Wraps `performance.mark` / `performance.measure` around the
// keystroke → kernel-apply path so a developer running `pnpm
// tauri:dev` with the `VITE_NEXUS_PERF_TYPING` env var set sees a
// per-keystroke timing series in the browser's Performance panel.
// No production-path cost: every helper short-circuits to a no-op
// when the env var isn't `'1'`.
//
// The BL-122 happy-dom harness covers the editor-engine half of
// this same pipeline (CM6 → StateField/ViewPlugin → DOM commit on
// a stub layout engine). This hook fills in the Tauri-IPC + real-
// paint + React-commit pieces for whoever is profiling a real
// session. The full WDIO-Tauri-driven scenarios that would automate
// the same measurement remain a deferred follow-up (BL-127 DoD
// bullet 1).

/** Stable id used in `performance.mark` / `performance.measure`. */
const MARK_START = 'nexus.typing.start'
const MARK_END = 'nexus.typing.end'
const MEASURE_NAME = 'nexus.typing.latency'

/** Counter so two overlapping keystrokes don't collide on the
 *  mark name. Reset on `clearTypingMarks` (rare; mostly for tests). */
let counter = 0

/** Cached env-var lookup so we pay the import.meta.env walk once.
 *  Set to `null` to force a re-read — handy for tests. */
let cachedEnabled: boolean | null = null

/** Read the `VITE_NEXUS_PERF_TYPING` env var. Truthy iff the value
 *  is the literal string `'1'`. Cached after the first call. */
export function typingPerfEnabled(): boolean {
  if (cachedEnabled !== null) return cachedEnabled
  cachedEnabled = readEnv() === '1'
  return cachedEnabled
}

function readEnv(): string | undefined {
  // import.meta.env is the Vite-injected env on the renderer side.
  // Guarded with `typeof` so unit tests that import the module
  // under Node without Vite don't blow up.
  try {
    const env = (import.meta as { env?: Record<string, string | undefined> }).env
    return env?.VITE_NEXUS_PERF_TYPING
  } catch {
    return undefined
  }
}

/** Test-only override. `enabled = null` re-reads the env on the
 *  next `typingPerfEnabled()` call. */
export function __setTypingPerfEnabledForTest(enabled: boolean | null): void {
  cachedEnabled = enabled
}

/** Bracket a keystroke. Returns an `end()` callback that closes the
 *  measure. Captures a stable per-call id so overlapping dispatches
 *  (rare, but possible inside `dispatchChain`) don't collide.
 *
 *  Usage:
 *
 *  ```ts
 *  const end = beginKeystroke()
 *  await dispatchAndAwait(...)
 *  end()
 *  ```
 *
 *  Mid-path errors are fine; the caller may drop the end callback
 *  silently. A future cleanup pass can prune orphan marks, but the
 *  number of dropped keystrokes is bounded by the number of
 *  thrown-from-the-bridge events, which is tiny. */
export function beginKeystroke(): () => void {
  if (!typingPerfEnabled()) return noop
  const id = ++counter
  const start = `${MARK_START}.${id}`
  const end = `${MARK_END}.${id}`
  try {
    performance.mark(start)
  } catch {
    return noop
  }
  return () => {
    try {
      performance.mark(end)
      performance.measure(`${MEASURE_NAME}.${id}`, start, end)
    } catch {
      // `performance.mark` / `measure` throw if the start mark was
      // garbage-collected. Swallowing keeps the typing path safe;
      // the measurement is best-effort.
    }
  }
}

/** Helper for tests + dev tools: walk the latest N measures and
 *  return their durations in ms, oldest first. Returns an empty
 *  array when no measures exist (e.g. the env var was never set).
 *  Note: this reads from the browser's Performance buffer, which
 *  has a default 150-entry cap — old entries fall off automatically. */
export function recentMeasureDurationsMs(limit = 100): number[] {
  try {
    const entries = performance.getEntriesByType('measure') as PerformanceEntry[]
    return entries
      .filter((e) => e.name.startsWith(`${MEASURE_NAME}.`))
      .slice(-limit)
      .map((e) => e.duration)
  } catch {
    return []
  }
}

/** Clear every typing-perf mark + measure. Tests use this to reset
 *  state between cases. */
export function clearTypingMarks(): void {
  counter = 0
  try {
    // `clearMarks` / `clearMeasures` take an optional name. Without
    // a name they clear ALL entries — too aggressive for production
    // use, so we iterate the typing-prefixed ones only.
    const marks = performance.getEntriesByType('mark')
    for (const m of marks) {
      if (m.name.startsWith(`${MARK_START}.`) || m.name.startsWith(`${MARK_END}.`)) {
        performance.clearMarks(m.name)
      }
    }
    const measures = performance.getEntriesByType('measure')
    for (const m of measures) {
      if (m.name.startsWith(`${MEASURE_NAME}.`)) {
        performance.clearMeasures(m.name)
      }
    }
  } catch {
    // No-op on hosts without performance API.
  }
}

function noop(): void {
  // Intentionally empty. Returned by `beginKeystroke` when the env
  // var is off so the production hot path pays only a cached-bool
  // read.
}
