// shell/src/plugins/nexus/debugger/debuggerStore.test.ts
//
// BL-081 — unit tests for the debugger view-model.

import { describe, it, beforeEach } from 'node:test'
import assert from 'node:assert/strict'

import { useDebuggerStore } from './debuggerStore'
import type { DapKernelAPI } from './debuggerIpc'

interface CallRecord {
  command: string
  args: unknown
}

function makeKernel(
  replies: Partial<Record<string, unknown>> = {},
): { api: DapKernelAPI; calls: CallRecord[] } {
  const calls: CallRecord[] = []
  const api: DapKernelAPI = {
    async invoke<T>(_p: string, command: string, args?: unknown): Promise<T> {
      calls.push({ command, args })
      const r = replies[command]
      // Default replies for verbs we don't override per test — keep
      // the surface honest without forcing every test to stub every
      // verb.
      if (r === undefined) {
        if (command === 'launch') return {} as T
        if (command === 'configuration_done') return { ok: true } as T
        if (command === 'disconnect') return { ok: true } as T
        if (command === 'set_breakpoints')
          return {
            breakpoints: ((args as { breakpoints: { line: number }[] }).breakpoints).map((b) => ({
              verified: true,
              line: b.line,
            })),
          } as T
        if (command === 'threads') return { threads: [{ id: 1, name: 'main' }] } as T
        if (command === 'stack_trace') return { stackFrames: [] } as T
        if (command === 'scopes') return { scopes: [] } as T
        if (command === 'variables') return { variables: [] } as T
        if (command === 'evaluate') return { result: 'eval-result' } as T
        return { ok: true } as T
      }
      if (typeof r === 'function') {
        // Per-test reply overrides are stored as `unknown`; a function
        // entry is by convention `(args) => reply`.
        return (r as (args?: unknown) => unknown)(args) as T
      }
      return r as T
    },
  }
  return { api, calls }
}

beforeEach(() => {
  useDebuggerStore.getState().reset()
})

describe('debuggerStore — lifecycle', () => {
  it('startSession sets activeAdapter + running, calls launch + configurationDone', async () => {
    const { api, calls } = makeKernel()
    await useDebuggerStore.getState().startSession(api, {
      adapter: 'mock',
      program: '/bin/true',
    })
    const s = useDebuggerStore.getState()
    assert.equal(s.activeAdapter, 'mock')
    assert.equal(s.running, true)
    const names = calls.map((c) => c.command)
    assert.deepEqual(names, ['launch', 'configuration_done'])
  })

  it('startSession surfaces an error and clears activeAdapter on launch failure', async () => {
    const api: DapKernelAPI = {
      async invoke<T>(_p: string, command: string): Promise<T> {
        if (command === 'launch') throw new Error('boom')
        return {} as T
      },
    }
    await useDebuggerStore.getState().startSession(api, {
      adapter: 'mock',
      program: '/bin/true',
    })
    const s = useDebuggerStore.getState()
    assert.equal(s.activeAdapter, null)
    assert.equal(s.running, false)
    assert.ok(s.error?.includes('boom'))
  })

  it('startSession replays cached breakpoints before configuration_done', async () => {
    // Seed a breakpoint into the store before the session starts.
    useDebuggerStore.setState({
      breakpointsByPath: { '/x.rs': [{ line: 7 }] },
    })
    const { api, calls } = makeKernel()
    await useDebuggerStore.getState().startSession(api, {
      adapter: 'mock',
      program: '/bin/true',
    })
    const order = calls.map((c) => c.command)
    assert.deepEqual(order, ['launch', 'set_breakpoints', 'configuration_done'])
    // Args carry the seeded breakpoint.
    const sb = calls[1].args as {
      adapter: string
      source_path: string
      breakpoints: { line: number }[]
    }
    assert.equal(sb.source_path, '/x.rs')
    assert.deepEqual(sb.breakpoints, [{ line: 7 }])
  })

  it('endSession dispatches disconnect and resets session fields', async () => {
    const { api, calls } = makeKernel()
    await useDebuggerStore.getState().startSession(api, {
      adapter: 'mock',
      program: '/bin/true',
    })
    await useDebuggerStore.getState().endSession(api)
    assert.equal(useDebuggerStore.getState().activeAdapter, null)
    assert.ok(calls.some((c) => c.command === 'disconnect'))
  })

  it('markTerminated clears session fields without touching breakpoints', () => {
    useDebuggerStore.setState({
      activeAdapter: 'mock',
      running: true,
      threads: [{ id: 1, name: 'main' }],
      breakpointsByPath: { '/x.rs': [{ line: 1 }] },
    })
    useDebuggerStore.getState().markTerminated()
    const s = useDebuggerStore.getState()
    assert.equal(s.activeAdapter, null)
    assert.equal(s.running, false)
    assert.equal(s.threads.length, 0)
    // Breakpoints persist across sessions.
    assert.deepEqual(s.breakpointsByPath, { '/x.rs': [{ line: 1 }] })
  })
})

describe('debuggerStore — breakpoints', () => {
  it('toggleBreakpoint adds a line then removes it on the second call', async () => {
    const { api, calls } = makeKernel()
    useDebuggerStore.setState({ activeAdapter: 'mock' })
    await useDebuggerStore.getState().toggleBreakpoint(api, '/x.rs', 10)
    let s = useDebuggerStore.getState()
    assert.deepEqual(s.breakpointsByPath['/x.rs'], [{ line: 10 }])
    await useDebuggerStore.getState().toggleBreakpoint(api, '/x.rs', 10)
    s = useDebuggerStore.getState()
    assert.deepEqual(s.breakpointsByPath['/x.rs'], [])
    // Two IPC calls: add, then clear.
    const sbCalls = calls.filter((c) => c.command === 'set_breakpoints')
    assert.equal(sbCalls.length, 2)
  })

  it('toggleBreakpoint updates local state even without an active session', async () => {
    const { api, calls } = makeKernel()
    // No activeAdapter set — store still records the line, just
    // doesn't dispatch IPC.
    await useDebuggerStore.getState().toggleBreakpoint(api, '/y.rs', 3)
    assert.deepEqual(useDebuggerStore.getState().breakpointsByPath['/y.rs'], [
      { line: 3 },
    ])
    assert.equal(calls.length, 0)
  })

  it('clearBreakpointsForPath empties the bucket and pushes an empty set when connected', async () => {
    const { api, calls } = makeKernel()
    useDebuggerStore.setState({
      activeAdapter: 'mock',
      breakpointsByPath: { '/x.rs': [{ line: 1 }, { line: 7 }] },
    })
    await useDebuggerStore.getState().clearBreakpointsForPath(api, '/x.rs')
    assert.deepEqual(useDebuggerStore.getState().breakpointsByPath['/x.rs'], [])
    const sb = calls.find((c) => c.command === 'set_breakpoints')
    assert.ok(sb != null, 'expected set_breakpoints call')
    assert.deepEqual((sb!.args as { breakpoints: unknown[] }).breakpoints, [])
  })
})

describe('debuggerStore — control flow', () => {
  it('doContinue clears stopped state and dispatches with currentThread', async () => {
    const { api, calls } = makeKernel()
    useDebuggerStore.setState({
      activeAdapter: 'mock',
      currentThread: 7,
      stoppedReason: 'breakpoint',
      frames: [{ id: 1, name: 'f', line: 1, column: 1 }],
      scopes: [{ name: 'Locals', variablesReference: 100 }],
    })
    await useDebuggerStore.getState().doContinue(api)
    const s = useDebuggerStore.getState()
    assert.equal(s.stoppedReason, null)
    assert.equal(s.currentFrame, null)
    assert.deepEqual(s.frames, [])
    assert.deepEqual(s.scopes, [])
    const c = calls.find((x) => x.command === 'continue')
    assert.ok(c, 'expected continue call')
    assert.deepEqual(c!.args, { adapter: 'mock', thread_id: 7 })
  })

  it('doNext / doStepIn / doStepOut all dispatch with currentThread', async () => {
    const { api, calls } = makeKernel()
    useDebuggerStore.setState({ activeAdapter: 'mock', currentThread: 2 })
    await useDebuggerStore.getState().doNext(api)
    await useDebuggerStore.getState().doStepIn(api)
    await useDebuggerStore.getState().doStepOut(api)
    const names = calls.map((c) => c.command)
    assert.ok(names.includes('next'))
    assert.ok(names.includes('step_in'))
    assert.ok(names.includes('step_out'))
  })

  it('doContinue without active adapter is a no-op', async () => {
    const { api, calls } = makeKernel()
    await useDebuggerStore.getState().doContinue(api)
    assert.equal(calls.length, 0)
  })
})

describe('debuggerStore — refreshAfterStop', () => {
  it('refreshes threads, stack, scopes, watches; sets currentThread + frame', async () => {
    const { api, calls } = makeKernel({
      threads: { threads: [{ id: 9, name: 'worker' }] },
      stack_trace: {
        stackFrames: [
          { id: 50, name: 'top', line: 5, column: 0 },
          { id: 51, name: 'caller', line: 9, column: 0 },
        ],
      },
      scopes: {
        scopes: [{ name: 'Locals', variablesReference: 1000 }],
      },
    })
    useDebuggerStore.setState({ activeAdapter: 'mock' })
    useDebuggerStore.getState().addWatch('x')
    await useDebuggerStore.getState().refreshAfterStop(api, 9, 'breakpoint')
    const s = useDebuggerStore.getState()
    assert.equal(s.currentThread, 9)
    assert.equal(s.stoppedReason, 'breakpoint')
    assert.equal(s.threads.length, 1)
    assert.equal(s.frames.length, 2)
    assert.equal(s.currentFrame, 50)
    assert.equal(s.scopes.length, 1)
    // Watches re-evaluated against the top frame.
    const ev = calls.find((c) => c.command === 'evaluate')
    assert.ok(ev, 'expected evaluate call for watches')
    const evArgs = ev!.args as { expression: string; frame_id?: number }
    assert.equal(evArgs.expression, 'x')
    assert.equal(evArgs.frame_id, 50)
    assert.equal(s.watches[0].value, 'eval-result')
  })

  it('handles empty stack — no scopes call, no frame, no watch eval', async () => {
    const { api, calls } = makeKernel({
      stack_trace: { stackFrames: [] },
    })
    useDebuggerStore.setState({ activeAdapter: 'mock' })
    useDebuggerStore.getState().addWatch('y')
    await useDebuggerStore.getState().refreshAfterStop(api, 1, 'breakpoint')
    const s = useDebuggerStore.getState()
    assert.equal(s.currentFrame, null)
    assert.equal(s.scopes.length, 0)
    // No scopes call since no top frame, and no eval frame_id.
    assert.equal(
      calls.filter((c) => c.command === 'scopes').length,
      0,
    )
  })
})

describe('debuggerStore — watches and output', () => {
  it('addWatch dedupes, removeWatch removes', () => {
    useDebuggerStore.getState().addWatch('a')
    useDebuggerStore.getState().addWatch('a') // duplicate
    useDebuggerStore.getState().addWatch('b')
    assert.equal(useDebuggerStore.getState().watches.length, 2)
    useDebuggerStore.getState().removeWatch('a')
    assert.equal(useDebuggerStore.getState().watches.length, 1)
    assert.equal(useDebuggerStore.getState().watches[0].expression, 'b')
  })

  it('addWatch trims whitespace and ignores blank input', () => {
    useDebuggerStore.getState().addWatch('   ')
    useDebuggerStore.getState().addWatch('  spaced  ')
    const ws = useDebuggerStore.getState().watches
    assert.equal(ws.length, 1)
    assert.equal(ws[0].expression, 'spaced')
  })

  it('pushOutput appends and caps the log', () => {
    const cap = 1000
    for (let i = 0; i < cap + 200; i++) {
      useDebuggerStore.getState().pushOutput('stdout', `line ${i}\n`)
    }
    const out = useDebuggerStore.getState().output
    assert.equal(out.length, cap)
    // Last line is the most recent.
    assert.ok(out[out.length - 1].text.includes(`line ${cap + 199}`))
    // First line is the oldest still-retained line.
    assert.ok(out[0].text.includes(`line 200`))
  })

  it('reevaluateWatches captures evaluate errors on a per-row basis', async () => {
    const api: DapKernelAPI = {
      async invoke<T>(_p: string, command: string, args?: unknown): Promise<T> {
        if (command === 'evaluate') {
          const a = args as { expression: string }
          if (a.expression === 'broken') throw new Error('eval failed')
          return { result: 'ok-' + a.expression } as T
        }
        return {} as T
      },
    }
    useDebuggerStore.setState({ activeAdapter: 'mock', currentFrame: 1 })
    useDebuggerStore.getState().addWatch('good')
    useDebuggerStore.getState().addWatch('broken')
    await useDebuggerStore.getState().reevaluateWatches(api)
    const ws = useDebuggerStore.getState().watches
    const good = ws.find((w) => w.expression === 'good')!
    const broken = ws.find((w) => w.expression === 'broken')!
    assert.equal(good.value, 'ok-good')
    assert.equal(good.error, null)
    assert.equal(broken.value, null)
    assert.ok(broken.error?.includes('eval failed'))
  })
})
