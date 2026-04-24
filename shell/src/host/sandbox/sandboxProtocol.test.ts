// shell/src/host/sandbox/sandboxProtocol.test.ts
//
// WI-30b unit tests for the sandbox RPC protocol + router.
//
// Sibling-of-implementation; surfaced to the default `pnpm test` glob
// via `shell/tests/sandbox-protocol.test.ts` (mirrors the UriHandler /
// ExtensionHost shim pattern).
//
// Coverage (minimum 10 cases per the WI-30b brief):
//   1. Handshake succeeds with matching SANDBOX_PROTOCOL_VERSION.
//   2. Handshake rejects on version mismatch with `protocol_mismatch`.
//   3. Handshake rejects on missing protocolVersion (`dispatch_failed`).
//   4. request → response round-trip preserves correlation id.
//   5. Unknown method returns `unknown_method`.
//   6. `views.register` returns `unknown_method` with a PanelNode hint.
//   7. Capability-gated method: no FsRead → `capability_denied`.
//   8. Capability-gated method: with FsRead → success.
//   9. `kernel.on` subscription: registering id, event delivery, correct
//      routing back to the guest under the same subscriptionId.
//  10. `kernel.off` tears down the real host subscription.
//  11. `dispose()` tears down every tracked subscription.
//  12. Timeout: request that exceeds the configured window returns
//      `timeout` error with `retryable: true`.
//  13. Disposed router rejects further requests with `plugin_disposed`.
//  14. METHOD_CAPABILITY_MAP covers exactly the catalog keys.
//  15. Catalog method-name list matches the SandboxMethodCatalog type
//      keys (both directions — no orphan names).
//  16. Request before handshake is rejected with `dispatch_failed`.
//  17. Pre-handshake dispatch is safe; capability check isn't reached.

// @ts-expect-error tsc lib doesn't include node builtins
import { test } from 'node:test'
// @ts-expect-error tsc lib doesn't include node builtins
import assert from 'node:assert/strict'

import {
  SANDBOX_PROTOCOL_VERSION,
  SANDBOX_METHOD_NAMES,
  SANDBOX_REJECTED_METHODS,
  METHOD_CAPABILITY_MAP,
  SandboxRouter,
  makeHandshakeHello,
  makeRequest,
  type RpcEnvelope,
  type SandboxMethodName,
  type SandboxPort,
} from './index.ts'
import type { PluginAPI } from '../../types/plugin.ts'

// ─── Test plumbing ───────────────────────────────────────────────────────────
//
// Use an in-memory port pair rather than the native MessageChannel —
// the router only touches `postMessage` + `onmessage`, so a simple
// synchronous mock is both deterministic (no microtask interleaving
// surprises) and works on every Node version the repo targets.

interface MockPort extends SandboxPort {
  sent: unknown[]
  flushHandler(): void
}

function makePortPair(): { host: MockPort; guest: MockPort } {
  const host: MockPort = {
    sent: [],
    onmessage: null,
    postMessage(msg: unknown) {
      host.sent.push(msg)
      // Deliver asynchronously to mimic structured-clone + task hop.
      queueMicrotask(() => {
        if (guest.onmessage) {
          guest.onmessage({ data: msg } as MessageEvent)
        }
      })
    },
    flushHandler() { /* no-op; async via microtask */ },
  }
  const guest: MockPort = {
    sent: [],
    onmessage: null,
    postMessage(msg: unknown) {
      guest.sent.push(msg)
      queueMicrotask(() => {
        if (host.onmessage) {
          host.onmessage({ data: msg } as MessageEvent)
        }
      })
    },
    flushHandler() { /* no-op */ },
  }
  return { host, guest }
}

function makeApi(overrides: Partial<PluginAPI> = {}): PluginAPI {
  const unused = (name: string) => () => { throw new Error(`unmocked: ${name}`) }
  const base: PluginAPI = {
    commands: {
      register: unused('commands.register'),
      execute: unused('commands.execute'),
      all: unused('commands.all'),
    },
    views: { register: unused('views.register') },
    // @ts-expect-error — test stub; the router only calls the methods exercised.
    workspace: {},
    // @ts-expect-error — test stub.
    viewRegistry: {},
    context: {
      set: unused('context.set'),
      get: unused('context.get'),
      evaluate: unused('context.evaluate'),
    },
    events: {
      on: unused('events.on'),
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
    notifications: { show: unused('notifications.show') },
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
      invoke: unused('kernel.invoke'),
      on: unused('kernel.on'),
      available: unused('kernel.available'),
    },
    platform: {
      fs: {
        readText: unused('platform.fs.readText'),
        writeText: unused('platform.fs.writeText'),
        readDir: unused('platform.fs.readDir'),
        exists: unused('platform.fs.exists'),
        mkdir: unused('platform.fs.mkdir'),
        remove: unused('platform.fs.remove'),
        rename: unused('platform.fs.rename'),
      },
      dialog: {
        openFile: unused('platform.dialog.openFile'),
        openDirectory: unused('platform.dialog.openDirectory'),
        saveFile: unused('platform.dialog.saveFile'),
      } as PluginAPI['platform']['dialog'],
      window: {
        minimize: unused('platform.window.minimize'),
        toggleMaximize: unused('platform.window.toggleMaximize'),
        close: unused('platform.window.close'),
        isMaximized: unused('platform.window.isMaximized'),
        onResize: unused('platform.window.onResize'),
      },
      shell: { openExternal: unused('platform.shell.openExternal') },
    },
    activityBar: {
      addItem: unused('activityBar.addItem'),
      removeItem: unused('activityBar.removeItem'),
    },
    input: {
      prompt: unused('input.prompt'),
      confirm: unused('input.confirm'),
    },
    uri: { register: unused('uri.register') },
  }
  return { ...base, ...overrides } as PluginAPI
}

/** Wait a microtask turn so deferred postMessage delivery resolves. */
async function tick(n = 1): Promise<void> {
  for (let i = 0; i < n; i++) await Promise.resolve()
}

function latest<T = RpcEnvelope>(port: MockPort): T {
  assert.ok(port.sent.length > 0, 'expected at least one message on port')
  return port.sent[port.sent.length - 1] as T
}

function buildRouter(
  opts: { api?: PluginAPI; grants?: ReadonlySet<string>; defaultTimeoutMs?: number } = {}
) {
  const { host, guest } = makePortPair()
  const router = new SandboxRouter({
    pluginId: 'test.plugin',
    api: opts.api ?? makeApi(),
    grantedCaps: opts.grants ?? new Set(),
    port: host,
    defaultTimeoutMs: opts.defaultTimeoutMs ?? 30_000,
    warn: () => { /* silent under test */ },
  })
  return { router, host, guest }
}

async function completeHandshake(
  ctx: { router: SandboxRouter; host: MockPort; guest: MockPort },
  protocolVersion = SANDBOX_PROTOCOL_VERSION as number,
): Promise<RpcEnvelope> {
  ctx.guest.postMessage(
    makeHandshakeHello({ protocolVersion, apiVersion: 1, nonce: 'hs-1' }),
  )
  await tick(2)
  return latest(ctx.host)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

test('handshake succeeds with matching SANDBOX_PROTOCOL_VERSION', async () => {
  const ctx = buildRouter()
  const accept = await completeHandshake(ctx)
  assert.equal(accept.kind, 'handshake')
  const payload = accept.payload as { protocolVersion: number; methods: string[]; nonce: string }
  assert.equal(payload.protocolVersion, SANDBOX_PROTOCOL_VERSION)
  assert.equal(payload.nonce, 'hs-1')
  assert.ok(Array.isArray(payload.methods) && payload.methods.length > 0)
  ctx.router.dispose()
})

test('handshake rejects on version mismatch with protocol_mismatch', async () => {
  const ctx = buildRouter()
  const reject = await completeHandshake(ctx, 999)
  assert.equal(reject.kind, 'handshake')
  assert.ok(reject.error, 'reject must carry an error')
  assert.equal(reject.error!.kind, 'protocol_mismatch')
  assert.equal(reject.error!.retryable, false)
  ctx.router.dispose()
})

test('handshake rejects malformed hello with dispatch_failed', async () => {
  const ctx = buildRouter()
  ctx.guest.postMessage({
    id: 'hs-bad',
    direction: 'plugin-to-host',
    kind: 'handshake',
    payload: { apiVersion: 1 }, // missing protocolVersion
  })
  await tick(2)
  const frame = latest<RpcEnvelope>(ctx.host)
  assert.equal(frame.kind, 'handshake')
  assert.equal(frame.error?.kind, 'dispatch_failed')
  ctx.router.dispose()
})

test('request → response round-trip preserves correlation id', async () => {
  const ctx = buildRouter({
    api: makeApi({
      storage: {
        get: (k) => k === 'answer' ? '42' : null,
        set: () => { throw new Error('unused') },
        delete: () => { throw new Error('unused') },
        clear: () => { throw new Error('unused') },
      },
    }),
  })
  await completeHandshake(ctx)
  ctx.host.sent.length = 0
  ctx.guest.postMessage(makeRequest('req-1', 'storage.get', { key: 'answer' }))
  await tick(4)
  const resp = latest<RpcEnvelope>(ctx.host)
  assert.equal(resp.kind, 'response')
  assert.equal(resp.id, 'req-1')
  assert.equal(resp.payload, '42')
  assert.equal(resp.error, undefined)
  ctx.router.dispose()
})

test('unknown method returns unknown_method error', async () => {
  const ctx = buildRouter()
  await completeHandshake(ctx)
  ctx.host.sent.length = 0
  ctx.guest.postMessage(makeRequest('req-unk', 'bogus.method', {}))
  await tick(4)
  const resp = latest<RpcEnvelope>(ctx.host)
  assert.equal(resp.kind, 'response')
  assert.equal(resp.id, 'req-unk')
  assert.equal(resp.error?.kind, 'unknown_method')
  ctx.router.dispose()
})

test('views.register returns unknown_method with a PanelNode hint', async () => {
  const ctx = buildRouter()
  await completeHandshake(ctx)
  ctx.host.sent.length = 0
  ctx.guest.postMessage(makeRequest('req-vr', 'views.register', {}))
  await tick(4)
  const resp = latest<RpcEnvelope>(ctx.host)
  assert.equal(resp.error?.kind, 'unknown_method')
  assert.match(String(resp.error?.message), /PanelNode|registerPanel/)
  ctx.router.dispose()
})

test('capability-gated method without grant returns capability_denied', async () => {
  const ctx = buildRouter({ grants: new Set() }) // no FsRead
  await completeHandshake(ctx)
  ctx.host.sent.length = 0
  ctx.guest.postMessage(makeRequest('req-fs', 'platform.fs.readText', { path: '/etc/passwd' }))
  await tick(4)
  const resp = latest<RpcEnvelope>(ctx.host)
  assert.equal(resp.error?.kind, 'capability_denied')
  assert.match(String(resp.error?.message), /FsRead/)
  ctx.router.dispose()
})

test('capability-gated method with grant succeeds', async () => {
  let readPath = ''
  const ctx = buildRouter({
    grants: new Set(['FsRead']),
    api: makeApi({
      platform: {
        fs: {
          readText: async (path: string) => { readPath = path; return 'hello' },
          writeText: () => { throw new Error('x') },
          readDir: () => { throw new Error('x') },
          exists: () => { throw new Error('x') },
          mkdir: () => { throw new Error('x') },
          remove: () => { throw new Error('x') },
          rename: () => { throw new Error('x') },
        },
        dialog: {} as PluginAPI['platform']['dialog'],
        window: {} as PluginAPI['platform']['window'],
        shell: {} as PluginAPI['platform']['shell'],
      },
    }),
  })
  await completeHandshake(ctx)
  ctx.host.sent.length = 0
  ctx.guest.postMessage(makeRequest('req-fs-ok', 'platform.fs.readText', { path: '/tmp/a' }))
  await tick(4)
  const resp = latest<RpcEnvelope>(ctx.host)
  assert.equal(resp.error, undefined)
  assert.equal(resp.payload, 'hello')
  assert.equal(readPath, '/tmp/a')
  ctx.router.dispose()
})

test('kernel.on subscription delivers events under the guest subscriptionId', async () => {
  let kernelCallback: ((topic: string, payload: unknown) => void) | null = null
  let hostUnsubCalls = 0
  const ctx = buildRouter({
    grants: new Set(),
    api: makeApi({
      kernel: {
        invoke: async () => { throw new Error('x') },
        on: (async (_prefix: string, handler: (topic: string, payload: unknown) => void) => {
          kernelCallback = handler
          return () => { hostUnsubCalls++ }
        }) as PluginAPI['kernel']['on'],
        available: async () => true,
      },
    }),
  })
  await completeHandshake(ctx)
  ctx.host.sent.length = 0

  ctx.guest.postMessage(makeRequest('req-on', 'kernel.on', {
    topicPrefix: 'cool.',
    handlerSub: 'sub-42',
  }))
  await tick(4)

  // Response echoes the subscription id.
  const resp = latest<RpcEnvelope>(ctx.host)
  assert.equal(resp.kind, 'response')
  assert.equal(resp.id, 'req-on')
  assert.deepEqual(resp.payload, { subscriptionId: 'sub-42' })
  assert.equal(ctx.router.subscriptionCount, 1)

  // Now fire a kernel event. The router must post an `event` envelope
  // carrying the subscriptionId so the guest can demux by id.
  ctx.host.sent.length = 0
  assert.ok(kernelCallback, 'kernel.on handler must have been captured')
  kernelCallback!('cool.update', { hello: 'world' })
  await tick(2)

  const evt = latest<RpcEnvelope>(ctx.host)
  assert.equal(evt.kind, 'event')
  assert.equal(evt.id, 'sub-42')
  assert.equal(evt.method, 'kernel.on')
  assert.deepEqual(evt.payload, { topic: 'cool.update', payload: { hello: 'world' } })

  // kernel.off tears down the real host subscription.
  ctx.guest.postMessage(makeRequest('req-off', 'kernel.off', { subscriptionId: 'sub-42' }))
  await tick(4)
  assert.equal(hostUnsubCalls, 1)
  assert.equal(ctx.router.subscriptionCount, 0)
  ctx.router.dispose()
})

test('dispose() tears down every tracked subscription', async () => {
  let unsubCalls = 0
  const ctx = buildRouter({
    api: makeApi({
      kernel: {
        invoke: async () => { throw new Error('x') },
        on: async () => () => { unsubCalls++ },
        available: async () => true,
      },
    }),
  })
  await completeHandshake(ctx)
  ctx.guest.postMessage(makeRequest('r1', 'kernel.on', { topicPrefix: 'a.', handlerSub: 's-a' }))
  ctx.guest.postMessage(makeRequest('r2', 'kernel.on', { topicPrefix: 'b.', handlerSub: 's-b' }))
  await tick(4)
  assert.equal(ctx.router.subscriptionCount, 2)
  ctx.router.dispose()
  assert.equal(unsubCalls, 2)
  assert.equal(ctx.router.subscriptionCount, 0)
  assert.equal(ctx.router.isDisposed, true)
})

test('timeout: request that exceeds window returns timeout error', async () => {
  const ctx = buildRouter({
    defaultTimeoutMs: 5,
    grants: new Set(['IpcCall']),
    api: makeApi({
      kernel: {
        invoke: () => new Promise(() => { /* never resolves */ }),
        on: async () => () => {},
        available: async () => true,
      },
    }),
  })
  await completeHandshake(ctx)
  ctx.host.sent.length = 0
  ctx.guest.postMessage(makeRequest('req-slow', 'kernel.invoke', {
    pluginId: 'other',
    commandId: 'x',
    args: {},
  }))
  // Wait past the 5ms window.
  await new Promise((r) => setTimeout(r, 25))
  const resp = latest<RpcEnvelope>(ctx.host)
  assert.equal(resp.kind, 'response')
  assert.equal(resp.id, 'req-slow')
  assert.equal(resp.error?.kind, 'timeout')
  assert.equal(resp.error?.retryable, true)
  ctx.router.dispose()
})

test('disposed router rejects further requests with plugin_disposed', async () => {
  const ctx = buildRouter()
  await completeHandshake(ctx)
  ctx.router.dispose()
  ctx.host.sent.length = 0
  // Direct handle() call — ports are closed on dispose.
  await ctx.router.handle(makeRequest('req-late', 'storage.get', { key: 'x' }))
  const resp = ctx.host.sent.find((m): m is RpcEnvelope =>
    !!m && typeof m === 'object' && (m as RpcEnvelope).kind === 'response'
  )
  assert.ok(resp, 'disposed router must still emit an error response')
  assert.equal(resp!.error?.kind, 'plugin_disposed')
})

test('METHOD_CAPABILITY_MAP covers exactly the catalog keys', () => {
  const mapKeys = new Set(Object.keys(METHOD_CAPABILITY_MAP))
  const catalogKeys = new Set<string>(SANDBOX_METHOD_NAMES)
  for (const k of catalogKeys) {
    assert.ok(mapKeys.has(k), `capability map missing entry for ${k}`)
  }
  for (const k of mapKeys) {
    assert.ok(catalogKeys.has(k), `capability map has orphan entry ${k}`)
  }
})

test('SANDBOX_METHOD_NAMES exposes every catalog key the router accepts', () => {
  // Every name the handshake advertises must be known to the catalog;
  // every rejected method must NOT appear in the name list.
  for (const name of SANDBOX_METHOD_NAMES) {
    assert.ok(
      name in METHOD_CAPABILITY_MAP,
      `advertised method ${name} has no capability-map entry`,
    )
  }
  for (const rejected of Object.keys(SANDBOX_REJECTED_METHODS)) {
    assert.equal(
      SANDBOX_METHOD_NAMES.includes(rejected as SandboxMethodName),
      false,
      `rejected method ${rejected} must NOT appear in SANDBOX_METHOD_NAMES`,
    )
  }
})

test('request before handshake is rejected with dispatch_failed', async () => {
  const ctx = buildRouter()
  ctx.guest.postMessage(makeRequest('early', 'storage.get', { key: 'x' }))
  await tick(4)
  const resp = latest<RpcEnvelope>(ctx.host)
  assert.equal(resp.kind, 'response')
  assert.equal(resp.error?.kind, 'dispatch_failed')
  assert.match(String(resp.error?.message), /handshake/)
  ctx.router.dispose()
})

test('dispatch errors from the backing API surface surface as dispatch_failed', async () => {
  const ctx = buildRouter({
    grants: new Set(['FsRead']),
    api: makeApi({
      platform: {
        fs: {
          readText: async () => { throw new Error('disk on fire') },
          writeText: () => { throw new Error('x') },
          readDir: () => { throw new Error('x') },
          exists: () => { throw new Error('x') },
          mkdir: () => { throw new Error('x') },
          remove: () => { throw new Error('x') },
          rename: () => { throw new Error('x') },
        },
        dialog: {} as PluginAPI['platform']['dialog'],
        window: {} as PluginAPI['platform']['window'],
        shell: {} as PluginAPI['platform']['shell'],
      },
    }),
  })
  await completeHandshake(ctx)
  ctx.host.sent.length = 0
  ctx.guest.postMessage(makeRequest('req-err', 'platform.fs.readText', { path: '/x' }))
  await tick(4)
  const resp = latest<RpcEnvelope>(ctx.host)
  assert.equal(resp.error?.kind, 'dispatch_failed')
  assert.match(String(resp.error?.message), /disk on fire/)
  ctx.router.dispose()
})

test('sendEvent is a no-op after dispose', () => {
  const ctx = buildRouter()
  ctx.router.dispose()
  ctx.host.sent.length = 0
  ctx.router.sendEvent('kernel.on', 'sub-dead', { noise: true })
  assert.equal(ctx.host.sent.length, 0)
})
