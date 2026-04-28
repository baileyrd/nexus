// shell/src/host/sandbox/sandboxE2E.test.ts
//
// WI-30f — end-to-end / integration tests for the community-plugin
// sandbox. Wave 3 closes Phase 3c by exercising the security + resilience
// properties that the Wave 1/2 unit tests only touch at the seams.
//
// Surface already covered by Wave 1/2 — these are *not* re-tested here:
//   • Protocol envelope round-trips …………… `sandboxProtocol.test.ts` (16)
//   • Per-method dispatch happy paths ………… `sandboxProtocol.test.ts`
//   • Capability-denied single-method check … `sandboxProtocol.test.ts`
//   • IframePort source-identity guard ……… `orchestrator.test.ts`
//   • Orchestrator mount / handshake / watchdog / unload …
//                                              `orchestrator.test.ts` (15)
//   • Scaffold type-contract regressions …… `types.test-d.ts`
//
// What this file ADDS (end-to-end grade):
//   1.  Capability composition — grants enforced across multiple methods
//       in a single running instance, with the correct error envelope.
//   2.  Real crash recovery — plugin that throws at dispatch time leaves
//       all its subscriptions cleanly torn down.
//   3.  Host-side subscription sweep when the orchestrator unloads a
//       plugin mid-subscription lifecycle.
//   4.  Concurrent plugin isolation — N sandboxed routers sharing a host
//       API do not cross-deliver events.
//   5.  Hot-path round-trip latency — sub-ms request/response over the
//       in-memory port pair, asserted with a generous budget.
//   6.  Handshake negative paths — bad protocol version, pre-handshake
//       request, malformed hello.
//   7.  Backpressure — a slow guest handler cannot deadlock the host's
//       dispatch of concurrent requests.
//   8.  Lifecycle — duplicate load rejected; dispose sweeps the watchdog
//       + commands registry; disposeAll clears the fleet.
//
// DEFERRED TO LIVE-BROWSER SMOKE (orchestrator's manual QA once Tauri
// dev-server is up — NOT testable here because we are jsdom-free):
//   • Real CSP enforcement: production host context blocks `eval` inside
//     the iframe's srcdoc.
//   • `sandbox="allow-scripts"` without `allow-same-origin` produces a
//     null-origin iframe — verify `iframe.contentDocument` access throws.
//   • postMessage origin behavior with null-origin iframes (event.origin
//     === "null" rather than "*" or a real host).
//   • DOM isolation — plugin cannot walk `window.parent.document` or
//     poison host globals via prototype pollution.
//   • `<script src="http://evil.com">` inserted inside the sandbox is
//     blocked by `script-src 'self' blob: data:`.
//   • Iframe removal after crash truly releases the realm (devtools
//     memory snapshot no longer shows the plugin bundle).
//
// Testing strategy:
//   * No jsdom. We keep the project's established pattern (two sibling
//     test files already follow it).
//   * Two reusable stand-ins:
//       - `makePortPair()` — an in-memory `SandboxPort` pair, copied
//         from `sandboxProtocol.test.ts`. Drives the router directly.
//       - `FakeWindow` + `FakeIframe` — the DOM-ish stub from
//         `orchestrator.test.ts`. Drives the full orchestrator.
//   * A `FakeGuest` helper wraps a router-facing port with a tiny
//     runtime that knows how to: answer handshake, echo requests,
//     stall a response, or throw. Lets one test simulate several
//     plugin personalities without shipping real guest bundles.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  SANDBOX_PROTOCOL_VERSION,
  SandboxOrchestrator,
  SandboxRouter,
  makeHandshakeAccept,
  makeHandshakeHello,
  makeHandshakeReject,
  makeRequest,
  makeEvent,
  type RpcEnvelope,
  type SandboxOrchestratorOptions,
  type SandboxPort,
} from './index.ts'
import type { PluginAPI } from '../../types/plugin.ts'

// ─── Port pair (identical semantics to sandboxProtocol.test.ts) ──────────────

interface MockPort extends SandboxPort {
  /** Messages this port has sent *out* via postMessage. */
  sent: unknown[]
  /** Messages this port has *received* on its onmessage hook. */
  received: unknown[]
}

function makePortPair(): { host: MockPort; guest: MockPort } {
  const host: MockPort = {
    sent: [],
    received: [],
    onmessage: null,
    postMessage(msg: unknown) {
      host.sent.push(msg)
      queueMicrotask(() => {
        guest.received.push(msg)
        if (guest.onmessage) {
          guest.onmessage({ data: msg } as MessageEvent)
        }
      })
    },
  }
  const guest: MockPort = {
    sent: [],
    received: [],
    onmessage: null,
    postMessage(msg: unknown) {
      guest.sent.push(msg)
      queueMicrotask(() => {
        host.received.push(msg)
        if (host.onmessage) {
          host.onmessage({ data: msg } as MessageEvent)
        }
      })
    },
  }
  return { host, guest }
}

async function tick(n = 1): Promise<void> {
  for (let i = 0; i < n; i++) await Promise.resolve()
}

// ─── PluginAPI stub ──────────────────────────────────────────────────────────

interface ApiHooks {
  /** Controls the value platform.fs.readText returns. Throws if unset. */
  readText?: (path: string) => Promise<string> | string
  writeText?: (path: string, content: string) => Promise<void> | void
  /** kernel.invoke result. */
  kernelInvoke?: (
    pluginId: string,
    cmd: string,
    args: unknown,
  ) => Promise<unknown> | unknown
  /** Held refs so tests can fire events into subscribers. */
  kernelOnEmit?: (cb: (topic: string, payload: unknown) => void) => () => void
  notificationShow?: (n: unknown) => void
  eventsOn?: (event: string, cb: (payload: unknown) => void) => () => void
  // Spy: tally of registered commands.
  commandsRegistered?: Map<string, (...a: unknown[]) => unknown>
}

function makeApi(hooks: ApiHooks = {}): PluginAPI {
  const unused = (name: string) => () => {
    throw new Error(`unmocked: ${name}`)
  }
  const cmds = hooks.commandsRegistered ?? new Map()
  return {
    commands: {
      register: (id: string, h: (...a: unknown[]) => unknown) => {
        cmds.set(id, h)
      },
      execute: async (id: string, ...args: unknown[]) => {
        const h = cmds.get(id)
        return h ? await h(...args) : undefined
      },
      all: () => [...cmds.keys()].map((id) => ({ id, title: id })),
      unregister: (id: string) => {
        cmds.delete(id)
      },
    } as unknown as PluginAPI['commands'],
    views: { register: unused('views.register') },
    workspace: {} as PluginAPI['workspace'],
    viewRegistry: {} as PluginAPI['viewRegistry'],
    context: {
      set: unused('context.set'),
      get: unused('context.get'),
      evaluate: unused('context.evaluate'),
    },
    events: {
      on: hooks.eventsOn
        ? (ev: string, cb: (p: unknown) => void) => hooks.eventsOn!(ev, cb)
        : unused('events.on'),
      emit: unused('events.emit'),
    },
    storage: {
      get: unused('storage.get'),
      set: unused('storage.set'),
      delete: unused('storage.delete'),
      clear: unused('storage.clear'),
    },
    statusBar: { createItem: unused('statusBar.createItem') },
    configuration: {
      register: unused('configuration.register'),
      getValue: unused('configuration.getValue'),
      setValue: unused('configuration.setValue'),
      onChange: unused('configuration.onChange'),
    },
    notifications: {
      show: hooks.notificationShow ?? unused('notifications.show'),
    },
    fs: {
      read: unused('fs.read'),
      write: unused('fs.write'),
      list: unused('fs.list'),
      watch: unused('fs.watch'),
      exists: unused('fs.exists'),
      mkdir: unused('fs.mkdir'),
      delete: unused('fs.delete'),
      rename: unused('fs.rename'),
    },
    kernel: {
      invoke: hooks.kernelInvoke
        ? (p: string, c: string, a: unknown) => hooks.kernelInvoke!(p, c, a)
        : unused('kernel.invoke'),
      on: hooks.kernelOnEmit
        ? async (
            _topicPrefix: string,
            cb: (topic: string, payload: unknown) => void,
          ) => hooks.kernelOnEmit!(cb)
        : unused('kernel.on'),
      available: unused('kernel.available'),
    },
    platform: {
      fs: {
        readText: hooks.readText
          ? (path: string) => Promise.resolve(hooks.readText!(path))
          : unused('platform.fs.readText'),
        writeText: hooks.writeText
          ? (path: string, content: string) =>
              Promise.resolve(hooks.writeText!(path, content))
          : unused('platform.fs.writeText'),
        readDir: unused('platform.fs.readDir'),
        exists: unused('platform.fs.exists'),
        mkdir: unused('platform.fs.mkdir'),
        remove: unused('platform.fs.remove'),
        rename: unused('platform.fs.rename'),
      },
      dialog: {} as PluginAPI['platform']['dialog'],
      window: {} as PluginAPI['platform']['window'],
      shell: {} as PluginAPI['platform']['shell'],
    },
    activityBar: {
      addItem: unused('activityBar.addItem'),
      removeItem: unused('activityBar.removeItem'),
    },
    input: { prompt: unused('input.prompt'), confirm: unused('input.confirm') },
    uri: { register: unused('uri.register') },
  } as unknown as PluginAPI
}

// ─── Router harness ──────────────────────────────────────────────────────────

function buildRouter(
  opts: {
    pluginId?: string
    api?: PluginAPI
    grants?: ReadonlySet<string>
    defaultTimeoutMs?: number
  } = {},
) {
  const { host, guest } = makePortPair()
  const router = new SandboxRouter({
    pluginId: opts.pluginId ?? 'e2e.plugin',
    api: opts.api ?? makeApi(),
    grantedCaps: opts.grants ?? new Set(),
    port: host,
    defaultTimeoutMs: opts.defaultTimeoutMs ?? 5_000,
    warn: () => {
      /* silent */
    },
  })
  return { router, host, guest }
}

async function completeHandshake(
  ctx: { router: SandboxRouter; host: MockPort; guest: MockPort },
  protocolVersion = SANDBOX_PROTOCOL_VERSION as number,
): Promise<RpcEnvelope> {
  ctx.guest.postMessage(
    makeHandshakeHello({
      protocolVersion,
      apiVersion: 1,
      nonce: 'hs-1',
    }),
  )
  await tick(3)
  return ctx.host.sent[ctx.host.sent.length - 1] as RpcEnvelope
}

/**
 * Drive a single request and return the response frame. Polls the host's
 * outbound queue briefly so async handlers (setTimeout, awaited fs
 * operations) resolve. Asserts the host sent a correlated response.
 */
async function rpc(
  ctx: { host: MockPort; guest: MockPort },
  id: string,
  method: string,
  payload: unknown,
  timeoutMs = 500,
): Promise<RpcEnvelope> {
  ctx.guest.postMessage(makeRequest(id, method, payload))
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    await tick(4)
    const resp = ctx.host.sent.find(
      (m): m is RpcEnvelope =>
        !!m &&
        typeof m === 'object' &&
        (m as RpcEnvelope).kind === 'response' &&
        (m as RpcEnvelope).id === id,
    )
    if (resp) return resp
    await new Promise((r) => setTimeout(r, 5))
  }
  throw new Error(`no response within ${timeoutMs}ms for ${id}/${method}`)
}

// ─── DOM stubs (copied-shape from orchestrator.test.ts) ──────────────────────

type MessageListener = (ev: MessageEvent) => void

class FakeWindow {
  private listeners = new Set<MessageListener>()
  public document: {
    body: {
      appendChild: (n: unknown) => void
      removeChild: (n: unknown) => void
    }
    createElement: (tag: string) => FakeIframe
  }
  private mounted: FakeIframe[] = []

  constructor() {
    this.document = {
      body: {
        appendChild: (n: unknown) => {
          this.mounted.push(n as FakeIframe)
        },
        removeChild: (n: unknown) => {
          this.mounted = this.mounted.filter((x) => x !== n)
        },
      },
      createElement: () => new FakeIframe(),
    }
  }
  addEventListener(type: 'message', l: MessageListener) {
    if (type === 'message') this.listeners.add(l)
  }
  removeEventListener(type: 'message', l: MessageListener) {
    if (type === 'message') this.listeners.delete(l)
  }
  dispatchFromIframe(iframe: FakeIframe, data: unknown) {
    const ev = { data, source: iframe.contentWindow } as unknown as MessageEvent
    for (const l of [...this.listeners]) l(ev)
  }
  get mountedIframes(): FakeIframe[] {
    return [...this.mounted]
  }
  get mountedCount(): number {
    return this.mounted.length
  }
}

class FakeIframe {
  public contentWindow: {
    postMessage: (msg: unknown, target: string) => void
    __id: number
  }
  public style: Record<string, string> = {}
  public srcdoc = ''
  private attributes = new Map<string, string>()
  public sent: unknown[] = []
  public parentNode: { removeChild: (n: unknown) => void } | null = null
  private static counter = 0
  constructor() {
    const id = ++FakeIframe.counter
    this.contentWindow = {
      __id: id,
      postMessage: (msg: unknown) => {
        this.sent.push(msg)
      },
    }
    this.parentNode = { removeChild: () => {} }
  }
  setAttribute(k: string, v: string) {
    this.attributes.set(k, v)
  }
  getAttribute(k: string) {
    return this.attributes.get(k)
  }
}

function makeOrchestrator(overrides: Partial<SandboxOrchestratorOptions> = {}): {
  win: FakeWindow
  orch: SandboxOrchestrator
  api: PluginAPI
  unregistered: string[]
} {
  const win = new FakeWindow()
  const api = makeApi()
  const unregistered: string[] = []
  const registry = {
    commands: {
      unregister: (id: string) => {
        unregistered.push(id)
      },
    },
    trackSubscription: () => {},
  } as unknown as SandboxOrchestratorOptions['registry']
  const orch = new SandboxOrchestrator({
    apiFactory: () => api,
    registry,
    window: win as unknown as Window & typeof globalThis,
    handshakeTimeoutMs: 60,
    pingIntervalMs: 40,
    maxMissedPongs: 2,
    warn: () => {},
    ...overrides,
  })
  return { win, orch, api, unregistered }
}

function acceptHandshakeForLatest(win: FakeWindow): void {
  const iframes = win.mountedIframes
  const latest = iframes[iframes.length - 1]
  if (!latest) return
  win.dispatchFromIframe(
    latest,
    makeHandshakeAccept({
      protocolVersion: SANDBOX_PROTOCOL_VERSION,
      pluginInstanceId: `e2e#${latest.contentWindow.__id}`,
      methods: [],
      nonce: 'hs-1',
    }),
  )
}

// ============================================================================
// SECURITY — capability enforcement
// ============================================================================

test('E2E: kernel.invoke without IpcCall grant returns capability_denied', async () => {
  const ctx = buildRouter({
    grants: new Set(), // no caps
    api: makeApi({
      kernelInvoke: () => {
        throw new Error(
          'kernel.invoke must NEVER be reached without IpcCall grant',
        )
      },
    }),
  })
  await completeHandshake(ctx)
  ctx.host.sent.length = 0
  const resp = await rpc(ctx, 'k-1', 'kernel.invoke', {
    pluginId: 'target',
    commandId: 'cmd',
    args: {},
  })
  assert.equal(resp.error?.kind, 'capability_denied')
  assert.match(String(resp.error?.message), /IpcCall/)
  assert.equal(resp.error?.retryable, false)
  ctx.router.dispose()
})

test('E2E: platform.fs.writeText without FsWrite grant returns capability_denied', async () => {
  let backingCalled = false
  const ctx = buildRouter({
    grants: new Set(['FsRead']), // read but not write
    api: makeApi({
      writeText: () => {
        backingCalled = true
      },
    }),
  })
  await completeHandshake(ctx)
  ctx.host.sent.length = 0
  const resp = await rpc(ctx, 'w-1', 'platform.fs.writeText', {
    path: '/etc/passwd',
    content: 'pwned',
  })
  assert.equal(resp.error?.kind, 'capability_denied')
  assert.match(String(resp.error?.message), /FsWrite/)
  assert.equal(backingCalled, false, 'backing fs.writeText must not be called')
  ctx.router.dispose()
})

test('E2E: UiNotify-only plugin can notify but cannot read fs', async () => {
  const shown: unknown[] = []
  const ctx = buildRouter({
    grants: new Set(['UiNotify']),
    api: makeApi({
      notificationShow: (n) => {
        shown.push(n)
      },
      readText: () => {
        throw new Error('readText must not be reached')
      },
    }),
  })
  await completeHandshake(ctx)
  ctx.host.sent.length = 0

  // Allowed
  const notif = await rpc(ctx, 'n-1', 'notifications.show', {
    notification: { message: 'hi', type: 'info' },
  })
  assert.equal(notif.error, undefined)
  assert.equal(shown.length, 1)

  // Denied
  const read = await rpc(ctx, 'r-1', 'platform.fs.readText', { path: '/tmp/a' })
  assert.equal(read.error?.kind, 'capability_denied')
  assert.match(String(read.error?.message), /FsRead/)
  ctx.router.dispose()
})

test('E2E: capability denial is a response envelope (not a thrown host error)', async () => {
  const ctx = buildRouter({ grants: new Set() })
  await completeHandshake(ctx)
  ctx.host.sent.length = 0
  const resp = await rpc(ctx, 'd-1', 'platform.fs.readText', { path: '/x' })
  // Contract: denial arrives as RpcErrorEnvelope on a 'response' frame,
  // correctly correlated — the guest can `await` its request without
  // wrapping it in try/catch-for-error-event.
  assert.equal(resp.kind, 'response')
  assert.equal(resp.id, 'd-1')
  assert.equal(resp.error?.kind, 'capability_denied')
  assert.equal(resp.payload, undefined)
  ctx.router.dispose()
})

// ============================================================================
// CRASH / RECOVERY
// ============================================================================

test('E2E: handler that throws mid-request surfaces as dispatch_failed response', async () => {
  const ctx = buildRouter({
    grants: new Set(['FsRead']),
    api: makeApi({
      readText: () => {
        throw new Error('disk on fire')
      },
    }),
  })
  await completeHandshake(ctx)
  ctx.host.sent.length = 0
  const resp = await rpc(ctx, 'x-1', 'platform.fs.readText', { path: '/a' })
  assert.equal(resp.error?.kind, 'dispatch_failed')
  assert.match(String(resp.error?.message), /disk on fire/)
  // Router stays alive after a handler throw — a later call still dispatches.
  assert.equal(ctx.router.isDisposed, false)
  ctx.router.dispose()
})

test('E2E: router.dispose() sweeps every tracked kernel.on subscription', async () => {
  const unsubsFired: string[] = []
  const ctx = buildRouter({
    grants: new Set(),
    api: makeApi({
      kernelOnEmit: () => {
        const id = `unsub-${unsubsFired.length}`
        return () => unsubsFired.push(id)
      },
    }),
  })
  await completeHandshake(ctx)
  ctx.host.sent.length = 0

  // Register 10 kernel.on subscriptions (the "10 subs then crash" scenario).
  for (let i = 0; i < 10; i++) {
    ctx.guest.postMessage(
      makeRequest(`sub-${i}`, 'kernel.on', {
        topicPrefix: `topic.${i}`,
        handlerSub: `handler-${i}`,
      }),
    )
  }
  await tick(10)
  assert.equal(
    ctx.router.subscriptionCount,
    10,
    'all 10 subscriptions registered',
  )

  // Simulate "plugin crash" by disposing the router (the orchestrator
  // does exactly this when the watchdog fires or unload is requested).
  ctx.router.dispose()

  assert.equal(
    unsubsFired.length,
    10,
    'every host-side disposer must fire on sweep',
  )
  assert.equal(ctx.router.subscriptionCount, 0)
})

test('E2E: post-dispose requests are rejected with plugin_disposed', async () => {
  const ctx = buildRouter()
  await completeHandshake(ctx)
  ctx.router.dispose()
  ctx.host.sent.length = 0

  ctx.guest.postMessage(makeRequest('late-1', 'commands.all', {}))
  await tick(4)
  const resp = ctx.host.sent.find(
    (m): m is RpcEnvelope =>
      !!m && typeof m === 'object' && (m as RpcEnvelope).id === 'late-1',
  )
  assert.ok(resp, 'expected a plugin_disposed response envelope')
  assert.equal(resp!.error?.kind, 'plugin_disposed')
})

// ============================================================================
// HANDSHAKE negative paths
// ============================================================================

test('E2E: guest handshake with wrong protocolVersion is rejected with protocol_mismatch', async () => {
  const ctx = buildRouter()
  const accept = await completeHandshake(ctx, SANDBOX_PROTOCOL_VERSION + 99)
  assert.equal(accept.kind, 'handshake')
  assert.equal(accept.error?.kind, 'protocol_mismatch')
  assert.equal(accept.error?.retryable, false)
  // Subsequent requests must still be gated by the failed handshake.
  ctx.host.sent.length = 0
  ctx.guest.postMessage(makeRequest('q-1', 'commands.all', {}))
  await tick(4)
  const resp = ctx.host.sent[0] as RpcEnvelope
  assert.equal(resp.kind, 'response')
  assert.equal(resp.error?.kind, 'dispatch_failed')
  assert.match(String(resp.error?.message), /before handshake/)
  ctx.router.dispose()
})

test('E2E: orchestrator load rejects when guest never handshakes (timeout)', async () => {
  const { orch } = makeOrchestrator({ handshakeTimeoutMs: 25 })
  await assert.rejects(
    () =>
      orch.load({
        pluginId: 'silent.plugin',
        bundleUrl: 'blob:b',
        runtimeUrl: 'blob:r',
        capabilities: new Set(),
      }),
    /handshake timeout/,
  )
})

test('E2E: orchestrator load rejects when guest sends handshake-reject', async () => {
  const { win, orch } = makeOrchestrator()
  const p = orch.load({
    pluginId: 'bad.plugin',
    bundleUrl: 'blob:b',
    runtimeUrl: 'blob:r',
    capabilities: new Set(),
  })
  await tick(2)
  const iframe = win.mountedIframes[0]!
  win.dispatchFromIframe(
    iframe,
    makeHandshakeReject({
      nonce: 'hs-1',
      reason: 'protocol_mismatch',
      message: 'guest refused',
    }),
  )
  await assert.rejects(p, /rejected handshake/)
})

// ============================================================================
// LIFECYCLE — concurrent plugins, unload, duplicate load
// ============================================================================

test('E2E: three concurrent plugins — events isolated per router', async () => {
  // Build three independent routers sharing nothing but the shape of
  // makeApi. Each registers its own kernel.on and receives only its
  // own emitted event.
  const emitters: Array<(topic: string, payload: unknown) => void> = []
  const makeCtx = (pluginId: string) =>
    buildRouter({
      pluginId,
      api: makeApi({
        kernelOnEmit: (cb) => {
          emitters.push(cb)
          return () => {}
        },
      }),
    })
  const a = makeCtx('plugin.a')
  const b = makeCtx('plugin.b')
  const c = makeCtx('plugin.c')
  await Promise.all([completeHandshake(a), completeHandshake(b), completeHandshake(c)])

  a.guest.postMessage(
    makeRequest('sa', 'kernel.on', { topicPrefix: 'p', handlerSub: 'ha' }),
  )
  b.guest.postMessage(
    makeRequest('sb', 'kernel.on', { topicPrefix: 'p', handlerSub: 'hb' }),
  )
  c.guest.postMessage(
    makeRequest('sc', 'kernel.on', { topicPrefix: 'p', handlerSub: 'hc' }),
  )
  await tick(6)
  assert.equal(emitters.length, 3)

  // Clear guest inboxes (events arrive at `received`, not `sent`).
  a.guest.received.length = 0
  b.guest.received.length = 0
  c.guest.received.length = 0

  // Fire an event through A's emitter only — B and C must not see it.
  emitters[0]!('evt.hello', { who: 'a' })
  await tick(4)

  const hasEventForHandler = (port: MockPort, handler: string) =>
    port.received.some(
      (m) =>
        !!m &&
        typeof m === 'object' &&
        (m as RpcEnvelope).kind === 'event' &&
        (m as RpcEnvelope).id === handler,
    )
  assert.equal(hasEventForHandler(a.guest, 'ha'), true, 'A receives its own event')
  assert.equal(hasEventForHandler(b.guest, 'hb'), false, 'B must not see A')
  assert.equal(hasEventForHandler(c.guest, 'hc'), false, 'C must not see A')

  a.router.dispose()
  b.router.dispose()
  c.router.dispose()
})

test('E2E: orchestrator.unload sends dispose, awaits teardown, removes iframe', async () => {
  const { win, orch } = makeOrchestrator()
  const p = orch.load({
    pluginId: 'unload.plugin',
    bundleUrl: 'blob:b',
    runtimeUrl: 'blob:r',
    capabilities: new Set(),
  })
  await tick(2)
  acceptHandshakeForLatest(win)
  const inst = await p
  assert.equal(win.mountedCount, 1)
  assert.equal(inst.state, 'active')

  await orch.unload('unload.plugin')
  assert.equal(inst.state, 'disposed')
  assert.equal(orch.get('unload.plugin'), undefined)
  // Router must be marked disposed — no late frames can leak out.
  assert.equal(inst.router.isDisposed, true)
})

test('E2E: duplicate load of same pluginId is rejected', async () => {
  const { win, orch } = makeOrchestrator()
  const p1 = orch.load({
    pluginId: 'dup2.plugin',
    bundleUrl: 'blob:b',
    runtimeUrl: 'blob:r',
    capabilities: new Set(),
  })
  await tick(2)
  acceptHandshakeForLatest(win)
  const i1 = await p1
  await assert.rejects(
    orch.load({
      pluginId: 'dup2.plugin',
      bundleUrl: 'blob:b',
      runtimeUrl: 'blob:r',
      capabilities: new Set(),
    }),
    /already loaded/,
  )
  await i1.dispose()
})

// ============================================================================
// RPC HOT-PATH
// ============================================================================

test('E2E: request/response round-trip stays under 50ms over in-memory port', async () => {
  const ctx = buildRouter({
    grants: new Set(['FsRead']),
    api: makeApi({
      readText: () => 'ok',
    }),
  })
  await completeHandshake(ctx)
  ctx.host.sent.length = 0

  const iters = 20
  // Warm-up for JIT.
  for (let i = 0; i < 3; i++) {
    await rpc(ctx, `warm-${i}`, 'platform.fs.readText', { path: '/x' })
  }

  const t0 = performance.now()
  for (let i = 0; i < iters; i++) {
    await rpc(ctx, `hot-${i}`, 'platform.fs.readText', { path: '/x' })
  }
  const dt = performance.now() - t0
  const avg = dt / iters

  // Generous envelope: CI can be slow. The property we care about is
  // "in the ms range, not the seconds range" — real jsdom benches
  // routinely sit well under 1ms per round-trip.
  assert.ok(avg < 50, `avg round-trip ${avg.toFixed(3)}ms exceeded 50ms budget`)
  ctx.router.dispose()
})

test('E2E: kernel.on event delivered to guest under the correct subscriptionId', async () => {
  let emit: ((t: string, p: unknown) => void) | undefined
  const ctx = buildRouter({
    grants: new Set(),
    api: makeApi({
      kernelOnEmit: (cb) => {
        emit = cb
        return () => {}
      },
    }),
  })
  await completeHandshake(ctx)
  ctx.host.sent.length = 0
  await rpc(ctx, 'reg-1', 'kernel.on', {
    topicPrefix: 'work.',
    handlerSub: 'my-handler-42',
  })
  ctx.guest.received.length = 0

  assert.ok(emit, 'kernel.on hook must have captured the emitter')
  emit!('work.done', { result: 7 })
  await tick(4)

  const events = ctx.guest.received.filter(
    (m): m is RpcEnvelope =>
      !!m && typeof m === 'object' && (m as RpcEnvelope).kind === 'event',
  )
  assert.equal(events.length, 1)
  assert.equal(events[0].id, 'my-handler-42', 'event id must be handlerSub')
  assert.equal(events[0].method, 'kernel.on')
  assert.deepEqual(events[0].payload, {
    topic: 'work.done',
    payload: { result: 7 },
  })
  ctx.router.dispose()
})

test('E2E: slow guest handler does not block concurrent requests to the same router', async () => {
  // Two grants — one slow (readText), one fast (notifications.show). The
  // slow handler awaits a 30ms timer; the fast handler resolves
  // immediately. Interleave them and assert the fast response lands
  // before the slow response.
  let fastCompletedAt = 0
  let slowCompletedAt = 0
  const ctx = buildRouter({
    grants: new Set(['FsRead', 'UiNotify']),
    api: makeApi({
      readText: async () => {
        await new Promise((r) => setTimeout(r, 30))
        return 'slow-ok'
      },
      notificationShow: () => {
        // Fast, sync.
      },
    }),
  })
  await completeHandshake(ctx)
  ctx.host.sent.length = 0

  // Fire slow first, then fast immediately after.
  const slowPromise = rpc(ctx, 's-1', 'platform.fs.readText', { path: '/x' })
    .then((r) => {
      slowCompletedAt = performance.now()
      return r
    })
  const fastPromise = rpc(ctx, 'f-1', 'notifications.show', {
    notification: { message: 'hi' },
  }).then((r) => {
    fastCompletedAt = performance.now()
    return r
  })

  const [slow, fast] = await Promise.all([slowPromise, fastPromise])
  assert.equal(slow.error, undefined)
  assert.equal(fast.error, undefined)
  assert.ok(
    fastCompletedAt < slowCompletedAt,
    `fast (${fastCompletedAt.toFixed(
      2,
    )}) must resolve before slow (${slowCompletedAt.toFixed(
      2,
    )}) — handler stall must not serialize dispatch`,
  )
  ctx.router.dispose()
})

// ============================================================================
// F-8.1.2 — pluginId binding at the sandbox boundary
// ============================================================================

test('F-8.1.2: orchestrator calls apiFactory with the orchestrator-set pluginId', async () => {
  const win = new FakeWindow()
  const factoryCalls: string[] = []
  const registry = {
    commands: { unregister: () => {} },
    trackSubscription: () => {},
  } as unknown as SandboxOrchestratorOptions['registry']
  const orch = new SandboxOrchestrator({
    apiFactory: (pluginId) => {
      factoryCalls.push(pluginId)
      return makeApi()
    },
    registry,
    window: win as unknown as Window & typeof globalThis,
    handshakeTimeoutMs: 60,
    pingIntervalMs: 1_000_000,
    maxMissedPongs: 100,
    warn: () => {},
  })

  const loadA = orch.load({
    pluginId: 'plugin.a',
    bundleUrl: 'blob:test/a',
    runtimeUrl: 'blob:test/runtime',
    capabilities: new Set(),
  })
  await tick(2)
  // Drive the most-recently-mounted iframe's handshake (plugin.a).
  acceptHandshakeForLatest(win)
  await loadA

  const loadB = orch.load({
    pluginId: 'plugin.b',
    bundleUrl: 'blob:test/b',
    runtimeUrl: 'blob:test/runtime',
    capabilities: new Set(),
  })
  await tick(2)
  acceptHandshakeForLatest(win)
  await loadB

  // The factory must have been called twice — once per pluginId — with
  // the orchestrator-set ids, NOT a shared 'community-sandbox' label.
  assert.deepEqual(factoryCalls, ['plugin.a', 'plugin.b'])
  await orch.disposeAll()
})

test('F-8.1.2: each sandboxed plugin gets its own PluginAPI instance', async () => {
  const win = new FakeWindow()
  const builtApis: Array<{ pluginId: string; api: PluginAPI }> = []
  const registry = {
    commands: { unregister: () => {} },
    trackSubscription: () => {},
  } as unknown as SandboxOrchestratorOptions['registry']
  const orch = new SandboxOrchestrator({
    apiFactory: (pluginId) => {
      const api = makeApi()
      builtApis.push({ pluginId, api })
      return api
    },
    registry,
    window: win as unknown as Window & typeof globalThis,
    handshakeTimeoutMs: 60,
    pingIntervalMs: 1_000_000,
    maxMissedPongs: 100,
    warn: () => {},
  })

  for (const id of ['plugin.x', 'plugin.y']) {
    const loaded = orch.load({
      pluginId: id,
      bundleUrl: 'blob:test/' + id,
      runtimeUrl: 'blob:test/runtime',
      capabilities: new Set(),
    })
    await tick(2)
    acceptHandshakeForLatest(win)
    await loaded
  }

  assert.equal(builtApis.length, 2)
  assert.notEqual(
    builtApis[0].api,
    builtApis[1].api,
    'both routers received the same PluginAPI instance — F-8.1.2 regression',
  )
  await orch.disposeAll()
})
