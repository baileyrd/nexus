// shell/src/registry/CommandRegistry.test.ts
//
// WI-35 — per-plugin crash quarantine for command handlers.
//
// Sibling-of-implementation; surfaced to the default `pnpm test` glob
// via `tests/command-registry.test.ts` (mirrors the UriHandlerRegistry
// + ExtensionHost shim pattern).
//
// Coverage (Q3 re-throw semantics):
//   - A handler that throws synchronously: execute() re-throws to the
//     caller; subsequent execute() calls for unrelated commands still
//     work; the registry emits `command:error` on the event bus.
//   - A handler that rejects asynchronously: same — the rejection
//     surfaces as an awaited error, not a swallowed one.
//   - Sibling handlers registered by other plugins stay callable after
//     a crash (the entry is not evicted).
//   - Unknown command: no handler wired → existing warn path preserved
//     (no throw, returns undefined) so the behaviour of manifest-only
//     stubs doesn't regress.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { CommandRegistry, CommandCancelledError } from './CommandRegistry.ts'
import { eventBus } from '../host/EventBus.ts'
import { useConfigStore } from '../stores/configStore.ts'

/**
 * OI-11 — temporarily lower the dispatch timeouts for a single test
 * and roll them back in a finally block. The persist middleware on
 * `useConfigStore` writes to localStorage when present, but in
 * node:test there's no localStorage so the mutation is in-memory only.
 */
function withTimeouts<T>(
  warnMs: number,
  cancelMs: number,
  fn: () => Promise<T>,
): Promise<T> {
  const store = useConfigStore.getState()
  store.set('shell.command.timeoutWarnMs', warnMs)
  store.set('shell.command.timeoutCancelMs', cancelMs)
  return fn().finally(() => {
    useConfigStore.getState().reset('shell.command.timeoutWarnMs')
    useConfigStore.getState().reset('shell.command.timeoutCancelMs')
  })
}

const sleep = (ms: number) => new Promise(r => setTimeout(r, ms))

test('WI-35 — sync handler that throws re-throws to caller', async () => {
  const reg = new CommandRegistry()
  reg.register('p.bad', 'cmd.boom', () => {
    throw new Error('boom-sync')
  })
  await assert.rejects(() => reg.execute('cmd.boom'), /boom-sync/)
})

test('WI-35 — async handler that rejects re-throws to caller', async () => {
  const reg = new CommandRegistry()
  reg.register('p.bad', 'cmd.reject', async () => {
    throw new Error('boom-async')
  })
  await assert.rejects(() => reg.execute('cmd.reject'), /boom-async/)
})

test('WI-35 — a throwing command does not break sibling commands', async () => {
  const reg = new CommandRegistry()
  reg.register('p.bad', 'cmd.boom', () => { throw new Error('boom') })
  reg.register('p.good', 'cmd.ok', () => 42)
  await assert.rejects(() => reg.execute('cmd.boom'))
  // Registry state unchanged — bad entry still present, good one still callable.
  assert.equal(reg.has('cmd.boom'), true)
  const r = await reg.execute('cmd.ok')
  assert.equal(r, 42)
})

test('WI-35 — execute() re-calls after a throw still work (no poisoning)', async () => {
  const reg = new CommandRegistry()
  let n = 0
  reg.register('p.flaky', 'cmd.flaky', () => {
    n++
    if (n === 1) throw new Error('first-call-fails')
    return n
  })
  await assert.rejects(() => reg.execute('cmd.flaky'))
  const r = await reg.execute('cmd.flaky')
  assert.equal(r, 2)
})

test('WI-35 — throwing handler emits command:error on the event bus', async () => {
  const reg = new CommandRegistry()
  const seen: Array<{ commandId: string; pluginId?: string; error: string }> = []
  const unsub = eventBus.on<{ commandId: string; pluginId?: string; error: string }>(
    'command:error',
    (e) => { seen.push(e) },
  )
  try {
    reg.register('p.err', 'cmd.err', () => {
      throw new Error('surface-me')
    })
    await assert.rejects(() => reg.execute('cmd.err'))
    assert.equal(seen.length, 1)
    assert.equal(seen[0].commandId, 'cmd.err')
    assert.equal(seen[0].pluginId, 'p.err')
    assert.match(seen[0].error, /surface-me/)
  } finally {
    unsub()
  }
})

test('WI-35 — unknown command: warn + undefined, no throw (regression guard)', async () => {
  const reg = new CommandRegistry()
  // Manifest-only (no handler) entries should still no-op with a warn —
  // the crash-quarantine try/catch must not change that contract.
  const r = await reg.execute('cmd.unknown')
  assert.equal(r, undefined)
})

// ─── OI-11 — UI-thread time budget on dispatch ───────────────────────────────

test('OI-11 — slow handler is hard-cancelled past the cancel threshold', async () => {
  const reg = new CommandRegistry()
  reg.register('p.slow', 'cmd.slow', async () => {
    await sleep(200)
    return 'done'
  })

  await withTimeouts(0, 30, async () => {
    await assert.rejects(
      () => reg.execute('cmd.slow'),
      (err: unknown) => err instanceof CommandCancelledError,
    )
  })
})

test('OI-11 — hard cancel emits command:cancelled with commandId + threshold', async () => {
  const reg = new CommandRegistry()
  reg.register('p.slow', 'cmd.slow2', async () => { await sleep(200) })

  const seen: Array<{ commandId: string; pluginId?: string; thresholdMs: number }> = []
  const off = eventBus.on<{ commandId: string; pluginId?: string; thresholdMs: number }>(
    'command:cancelled',
    (e) => { seen.push(e) },
  )
  try {
    await withTimeouts(0, 30, async () => {
      await assert.rejects(() => reg.execute('cmd.slow2'))
    })
  } finally {
    off()
  }

  assert.equal(seen.length, 1)
  assert.equal(seen[0].commandId, 'cmd.slow2')
  assert.equal(seen[0].pluginId, 'p.slow')
  assert.equal(seen[0].thresholdMs, 30)
})

test('OI-11 — hard cancel does NOT emit command:error (cancellation is not a crash)', async () => {
  const reg = new CommandRegistry()
  reg.register('p.slow', 'cmd.slow3', async () => { await sleep(200) })

  const errors: unknown[] = []
  const off = eventBus.on('command:error', (e) => { errors.push(e) })
  try {
    await withTimeouts(0, 20, async () => {
      await assert.rejects(() => reg.execute('cmd.slow3'))
    })
  } finally {
    off()
  }
  assert.equal(errors.length, 0, 'cancelled commands should not surface as errors')
})

test('OI-11 — fast handler completes normally and emits no cancellation', async () => {
  const reg = new CommandRegistry()
  reg.register('p.fast', 'cmd.fast', () => 7)

  const cancellations: unknown[] = []
  const off = eventBus.on('command:cancelled', (e) => { cancellations.push(e) })
  try {
    const r = await withTimeouts(0, 1000, () => reg.execute('cmd.fast'))
    assert.equal(r, 7)
  } finally {
    off()
  }
  assert.equal(cancellations.length, 0)
})

test('OI-11 — timeoutCancelMs=0 disables cancellation entirely', async () => {
  const reg = new CommandRegistry()
  reg.register('p.slow', 'cmd.slow4', async () => {
    await sleep(40)
    return 'still-here'
  })

  // 30 ms would normally cancel a 40 ms handler, but 0 means "off".
  await withTimeouts(0, 0, async () => {
    const r = await reg.execute('cmd.slow4')
    assert.equal(r, 'still-here')
  })
})

test('OI-11 — soft warn fires before cancel for a moderately slow handler', async () => {
  const reg = new CommandRegistry()
  reg.register('p.warn', 'cmd.warn', async () => {
    await sleep(40)
    return 'ok'
  })

  // Capture warns; the registry's warn message includes the command id.
  const warnings: string[] = []
  const orig = console.warn
  console.warn = (...args: unknown[]) => { warnings.push(args.join(' ')) }
  try {
    await withTimeouts(15, 200, async () => {
      const r = await reg.execute('cmd.warn')
      assert.equal(r, 'ok')
    })
  } finally {
    console.warn = orig
  }

  assert.ok(
    warnings.some(w => w.includes('cmd.warn') && w.includes('still pending')),
    `expected a soft-warn entry, got: ${JSON.stringify(warnings)}`,
  )
})

test('boot deadlock fix — kernel-boot commands are exempt from the hard cancel', async () => {
  // Regression: `nexus.workspace.open` boots the kernel (init_forge +
  // boot_kernel), which on a cold/debug build exceeds the 5s cancel
  // budget. Racing it against the cancel made the launcher tear down a
  // still-booting kernel and retry into a forge-lock deadlock. The
  // command id is in DEFAULT_NO_CANCEL_COMMANDS, so even a 20ms cancel
  // threshold must NOT cancel a 60ms "boot".
  const reg = new CommandRegistry()
  reg.register('nexus.workspace', 'nexus.workspace.open', async () => {
    await sleep(60)
    return 'booted'
  })

  const cancellations: unknown[] = []
  const off = eventBus.on('command:cancelled', (e) => { cancellations.push(e) })
  try {
    await withTimeouts(0, 20, async () => {
      const r = await reg.execute('nexus.workspace.open')
      assert.equal(r, 'booted', 'exempt command must run to completion')
    })
  } finally {
    off()
  }
  assert.equal(cancellations.length, 0, 'exempt command must not emit command:cancelled')
})

test('boot deadlock fix — non-exempt commands are still hard-cancelled', async () => {
  // Guard: the exemption is scoped to the named boot commands only —
  // an ordinary slow handler must still be cancelled as before.
  const reg = new CommandRegistry()
  reg.register('p.slow', 'cmd.ordinary', async () => { await sleep(200) })
  await withTimeouts(0, 20, async () => {
    await assert.rejects(
      () => reg.execute('cmd.ordinary'),
      (err: unknown) => err instanceof CommandCancelledError,
    )
  })
})

test('OI-11 — warnMs=0 disables the soft-warn path', async () => {
  const reg = new CommandRegistry()
  reg.register('p.silent', 'cmd.silent', async () => {
    await sleep(30)
  })

  const warnings: string[] = []
  const orig = console.warn
  console.warn = (...args: unknown[]) => { warnings.push(args.join(' ')) }
  try {
    await withTimeouts(0, 500, () => reg.execute('cmd.silent'))
  } finally {
    console.warn = orig
  }
  assert.equal(
    warnings.filter(w => w.includes('still pending')).length,
    0,
    'warnMs=0 should suppress the still-pending warn',
  )
})
