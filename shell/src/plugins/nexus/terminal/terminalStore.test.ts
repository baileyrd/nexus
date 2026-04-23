// shell/src/plugins/nexus/terminal/terminalStore.test.ts
//
// WI-12 (TS half) unit tests for the terminal stream store. Covers:
//   • handleStreamChunk routes bytes to the registered sink for the
//     right session id (no cross-contamination across sessions).
//   • Out-of-order chunks (seq gap) trigger lag-recovery via the
//     registered recoverFn — and post-recovery the store re-baselines
//     so the next chunk's seq becomes the new baseline (option a).
//   • Sink registration + unregistration is symmetric (mirrors the
//     PluginRegistry-tracked unsub pattern from Phase 1 wiring).
//   • Subscription forwarder shape — what api.kernel.on(prefix, …)
//     hands to handleStreamChunk — works end-to-end through a mock
//     api.kernel.on.
//
// Run from the shell/ package with:
//   node --import tsx --test \
//     shell/src/plugins/nexus/terminal/terminalStore.test.ts

// node:test + node:assert/strict aren't in the shell tsconfig's `lib`
// set (no `@types/node`). Use `@ts-expect-error` to keep tsc quiet
// without depending on a typings pkg — same pattern as
// savedCommandsStore.test.ts and aiStore.test.ts.
//
// @ts-expect-error tsc lib doesn't include node builtins
import { test } from 'node:test'
// @ts-expect-error tsc lib doesn't include node builtins
import assert from 'node:assert/strict'
import {
  useTerminalStore,
  type OutputStreamPayload,
  type RecoverFn,
} from './terminalStore.ts'

function reset(): void {
  const s = useTerminalStore.getState()
  s.setSession(null)
  s.setVisible(false)
  s.setRecoverFn(null)
  s.resetStreams()
}

function chunk(seq: number, bytes: number[]): OutputStreamPayload {
  return { seq, data: bytes, ts_ms: 0 }
}

/**
 * Yield to the microtask queue so any in-flight recoverFn promise
 * runs to completion. The store invokes recoverFn via .then(), so a
 * single Promise.resolve() is enough to drain the chain.
 */
async function flushMicrotasks(): Promise<void> {
  for (let i = 0; i < 4; i++) await Promise.resolve()
}

// ── routing ─────────────────────────────────────────────────────────────────

test('handleStreamChunk: writes bytes to the sink for the matching session', () => {
  reset()
  const captured: number[][] = []
  const unsub = useTerminalStore.getState().registerSink('s1', (b) => {
    captured.push(Array.from(b))
  })
  useTerminalStore.getState().handleStreamChunk('s1', chunk(1, [65, 66, 67]))
  assert.deepEqual(captured, [[65, 66, 67]])
  // lastSeq + lastCursor advance to mirror what was written.
  const stream = useTerminalStore.getState().streams['s1']
  assert.equal(stream.lastSeq, 1)
  assert.equal(stream.lastCursor, 3)
  unsub()
})

test('handleStreamChunk: chunk for an unknown session is silently dropped', () => {
  reset()
  // No sink registered.
  useTerminalStore.getState().handleStreamChunk('ghost', chunk(1, [1, 2]))
  // Stream state is still recorded (so the next chunk for the same
  // session correctly seq-checks), but no throw / no leak across
  // sessions.
  const stream = useTerminalStore.getState().streams['ghost']
  assert.ok(stream)
  assert.equal(stream.lastSeq, 1)
})

test('handleStreamChunk: multiple sessions stay isolated', () => {
  reset()
  const a: number[][] = []
  const b: number[][] = []
  useTerminalStore.getState().registerSink('a', (x) => a.push(Array.from(x)))
  useTerminalStore.getState().registerSink('b', (x) => b.push(Array.from(x)))
  useTerminalStore.getState().handleStreamChunk('a', chunk(1, [1]))
  useTerminalStore.getState().handleStreamChunk('b', chunk(1, [9]))
  useTerminalStore.getState().handleStreamChunk('a', chunk(2, [2]))
  useTerminalStore.getState().handleStreamChunk('b', chunk(2, [8]))
  assert.deepEqual(a, [[1], [2]])
  assert.deepEqual(b, [[9], [8]])
  // Per-session bookkeeping is independent.
  assert.equal(useTerminalStore.getState().streams['a'].lastSeq, 2)
  assert.equal(useTerminalStore.getState().streams['b'].lastSeq, 2)
  assert.equal(useTerminalStore.getState().streams['a'].lastCursor, 2)
  assert.equal(useTerminalStore.getState().streams['b'].lastCursor, 2)
})

test('registerSink: unregister is a no-op when a different sink has taken over', () => {
  reset()
  const a: number[][] = []
  const b: number[][] = []
  const unsubA = useTerminalStore.getState().registerSink('s1', (x) => a.push(Array.from(x)))
  // Replace sink (e.g. xterm remount).
  useTerminalStore.getState().registerSink('s1', (x) => b.push(Array.from(x)))
  // The old unsub should NOT clear the live sink.
  unsubA()
  useTerminalStore.getState().handleStreamChunk('s1', chunk(1, [42]))
  assert.deepEqual(a, [])
  assert.deepEqual(b, [[42]])
})

// ── lag recovery ────────────────────────────────────────────────────────────

test('handleStreamChunk: seq gap triggers recoverFn with the last byte cursor', async () => {
  reset()
  const written: number[][] = []
  useTerminalStore.getState().registerSink('s1', (b) => written.push(Array.from(b)))

  const recoverCalls: Array<{ id: string; cursor: number }> = []
  const recoverFn: RecoverFn = async (id, cursor) => {
    recoverCalls.push({ id, cursor })
    // Simulate read_raw_since returning 5 bytes since cursor=3.
    return { cursor: 8, data: new Uint8Array([10, 20, 30, 40, 50]) }
  }
  useTerminalStore.getState().setRecoverFn(recoverFn)

  // Establish baseline: seq=1, 3 bytes → lastCursor=3.
  useTerminalStore.getState().handleStreamChunk('s1', chunk(1, [1, 2, 3]))
  assert.equal(useTerminalStore.getState().streams['s1'].lastCursor, 3)

  // GAP — expect seq=2, get seq=4. Should trigger recoverFn(id='s1', cursor=3).
  useTerminalStore.getState().handleStreamChunk('s1', chunk(4, [99, 99]))
  // The gap-triggering chunk itself is dropped; only its presence
  // schedules the recovery.
  assert.equal(written.length, 1, 'gap chunk must NOT be written')

  // recoveryInFlight latches between the synchronous gap and the
  // async snapshot resolution.
  assert.equal(useTerminalStore.getState().streams['s1'].recoveryInFlight, true)

  await flushMicrotasks()

  // Snapshot bytes were handed to the sink and cursor jumped to 8.
  assert.deepEqual(recoverCalls, [{ id: 's1', cursor: 3 }])
  assert.deepEqual(written[1], [10, 20, 30, 40, 50])
  const after = useTerminalStore.getState().streams['s1']
  assert.equal(after.recoveryInFlight, false)
  assert.equal(after.lastCursor, 8)
  // Option (a): post-recovery, lastSeq is reset to 0 so the NEXT
  // chunk's seq becomes the new baseline regardless of value.
  assert.equal(after.lastSeq, 0)
})

test('handleStreamChunk: chunks arriving DURING recovery are dropped', async () => {
  reset()
  const written: number[][] = []
  useTerminalStore.getState().registerSink('s1', (b) => written.push(Array.from(b)))

  let resolveRecover!: (v: { cursor: number; data: Uint8Array } | null) => void
  const recoverFn: RecoverFn = () =>
    new Promise((res) => {
      resolveRecover = res
    })
  useTerminalStore.getState().setRecoverFn(recoverFn)

  useTerminalStore.getState().handleStreamChunk('s1', chunk(1, [1]))
  useTerminalStore.getState().handleStreamChunk('s1', chunk(5, [9])) // gap → recovery
  // While recovery is in flight, more chunks arrive — they must be
  // dropped because the snapshot will cover that range.
  useTerminalStore.getState().handleStreamChunk('s1', chunk(6, [9]))
  useTerminalStore.getState().handleStreamChunk('s1', chunk(7, [9]))
  assert.equal(written.length, 1, 'only the pre-gap chunk should have been written')

  resolveRecover({ cursor: 100, data: new Uint8Array([255]) })
  await flushMicrotasks()
  assert.deepEqual(written[1], [255])

  // After recovery, the very next chunk re-baselines (option a).
  useTerminalStore.getState().handleStreamChunk('s1', chunk(42, [77]))
  assert.deepEqual(written[2], [77])
  const after = useTerminalStore.getState().streams['s1']
  assert.equal(after.lastSeq, 42)
  assert.equal(after.lastCursor, 101)
})

test('handleStreamChunk: gap with no recoverFn wired writes the chunk + re-baselines', () => {
  reset()
  const written: number[][] = []
  useTerminalStore.getState().registerSink('s1', (b) => written.push(Array.from(b)))
  // No setRecoverFn — simulates pre-activate / shutdown windows.

  useTerminalStore.getState().handleStreamChunk('s1', chunk(1, [1]))
  useTerminalStore.getState().handleStreamChunk('s1', chunk(7, [2, 3]))
  assert.deepEqual(written, [[1], [2, 3]])
  const after = useTerminalStore.getState().streams['s1']
  assert.equal(after.lastSeq, 7)
  // Cursor advances by data.length even on the re-baseline path so
  // the next pump heartbeat doesn't double-write.
  assert.equal(after.lastCursor, 3)
})

test('advanceCursor: only moves forward, never backward', () => {
  reset()
  useTerminalStore.getState().registerSink('s1', () => {})
  useTerminalStore.getState().handleStreamChunk('s1', chunk(1, [1, 2, 3]))
  useTerminalStore.getState().advanceCursor('s1', 100)
  assert.equal(useTerminalStore.getState().streams['s1'].lastCursor, 100)
  // A later, smaller cursor (e.g. a stale pump response) is ignored.
  useTerminalStore.getState().advanceCursor('s1', 50)
  assert.equal(useTerminalStore.getState().streams['s1'].lastCursor, 100)
})

test('resetStreams: clears all per-session bookkeeping and sinks', () => {
  reset()
  const written: number[][] = []
  useTerminalStore.getState().registerSink('s1', (b) => written.push(Array.from(b)))
  useTerminalStore.getState().handleStreamChunk('s1', chunk(1, [1]))
  useTerminalStore.getState().resetStreams()
  assert.deepEqual(useTerminalStore.getState().streams, {})
  assert.deepEqual(useTerminalStore.getState().sinks, {})
  // After reset, a new chunk routes to nobody (sink was cleared).
  useTerminalStore.getState().handleStreamChunk('s1', chunk(1, [2]))
  assert.deepEqual(written, [[1]])
})

// ── subscribe / unsubscribe via api.kernel.on shape ─────────────────────────

test('api.kernel.on prefix forwarder: routes per-session chunks into the store', async () => {
  reset()
  const captured: Record<string, number[][]> = { a: [], b: [] }
  useTerminalStore.getState().registerSink('a', (x) => captured.a.push(Array.from(x)))
  useTerminalStore.getState().registerSink('b', (x) => captured.b.push(Array.from(x)))

  // Mock api.kernel.on the way Phase 1 wiring exposes it: takes a
  // prefix + (topic, payload) handler, returns an unsub. We capture
  // the handler so the test can drive it as if the kernel were
  // publishing events.
  type Handler = (topic: string, payload: OutputStreamPayload) => void
  let installed: { prefix: string; handler: Handler } | null = null
  const mockKernelOn = async (prefix: string, handler: Handler) => {
    installed = { prefix, handler }
    return () => {
      installed = null
    }
  }

  // Mirror what activate() does.
  const STREAM_TOPIC_PREFIX = 'com.nexus.terminal.output.'
  const unsub = await mockKernelOn(STREAM_TOPIC_PREFIX, (topic, payload) => {
    const sessionId = topic.slice(STREAM_TOPIC_PREFIX.length)
    if (!sessionId) return
    useTerminalStore.getState().handleStreamChunk(sessionId, payload)
  })
  assert.ok(installed, 'subscription must be installed')
  assert.equal(installed!.prefix, STREAM_TOPIC_PREFIX)

  // Drive two sessions through the same forwarder.
  installed!.handler('com.nexus.terminal.output.a', chunk(1, [1, 2]))
  installed!.handler('com.nexus.terminal.output.b', chunk(1, [9, 8]))
  installed!.handler('com.nexus.terminal.output.a', chunk(2, [3]))

  assert.deepEqual(captured.a, [[1, 2], [3]])
  assert.deepEqual(captured.b, [[9, 8]])

  // Unsubscribe is symmetric.
  unsub()
  assert.equal(installed, null)
})
