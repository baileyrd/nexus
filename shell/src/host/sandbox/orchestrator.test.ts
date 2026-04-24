// shell/src/host/sandbox/orchestrator.test.ts
//
// WI-30d — unit tests for the iframe orchestrator, IframePort adapter,
// and SandboxPanelView PanelNode renderer.
//
// The tests use a hand-rolled DOM-ish stub rather than jsdom because
// the repo already relies on plain Node tests (see `sandboxProtocol
// .test.ts` for the sibling pattern). The stub models exactly the
// surface `IframePort` + `SandboxOrchestrator` depend on: a Window
// with `addEventListener`/`removeEventListener`/`document`, and an
// iframe whose `contentWindow` is a `MessagePort`-ish relay.
//
// Coverage (10–15 cases per brief):
//   1. IframePort: delivers envelopes from the iframe's contentWindow.
//   2. IframePort: drops frames from an unrelated source.
//   3. IframePort: drops non-envelope frames.
//   4. IframePort: postMessage reaches the iframe's contentWindow.
//   5. IframePort: close() detaches the window listener + is idempotent.
//   6. Orchestrator: load() mounts an iframe + completes handshake.
//   7. Orchestrator: load() rejects on handshake timeout.
//   8. Orchestrator: load() rejects on guest-sent handshake reject.
//   9. Orchestrator: watchdog crashes after N missed pongs.
//  10. Orchestrator: pong resets the missed counter + keeps instance alive.
//  11. Orchestrator: unload() removes the iframe + disposes the router.
//  12. Orchestrator: dispose-all cleans every instance.
//  13. PanelNode renderer: walks a vstack/heading/button tree into React.
//  14. PanelNode renderer: button click dispatches api.commands.execute.
//  15. buildSandboxSrcDoc embeds runtime + bundle URLs safely.

// @ts-expect-error tsc lib doesn't include node builtins
import { test } from 'node:test'
// @ts-expect-error tsc lib doesn't include node builtins
import assert from 'node:assert/strict'

import {
  buildSandboxSrcDoc,
  IframePort,
  SandboxOrchestrator,
  renderPanelNode,
  type SandboxInstance,
  type SandboxOrchestratorOptions,
} from './index.ts'
import {
  SANDBOX_PROTOCOL_VERSION,
  makeHandshakeAccept,
  makeHandshakeReject,
  makeEvent,
  type RpcEnvelope,
} from '@nexus/extension-api'
import type { PluginAPI } from '../../types/plugin.ts'
import type { PanelNode } from '@nexus/extension-api'

// ─── DOM stubs ───────────────────────────────────────────────────────────────

type MessageListener = (ev: MessageEvent) => void

/**
 * Minimal `Window`-shaped stub that the orchestrator + port use. The
 * real DOM is irrelevant here: the tests drive message traffic by
 * calling `fakeWindow.dispatch(ev)` directly.
 */
class FakeWindow {
  private listeners = new Set<MessageListener>()
  public document: {
    body: { appendChild: (n: unknown) => void; removeChild: (n: unknown) => void }
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

  addEventListener(type: 'message', listener: MessageListener): void {
    if (type !== 'message') return
    this.listeners.add(listener)
  }

  removeEventListener(type: 'message', listener: MessageListener): void {
    if (type !== 'message') return
    this.listeners.delete(listener)
  }

  /** Test helper: deliver a message as if it came from the iframe. */
  dispatchFromIframe(iframe: FakeIframe, data: unknown): void {
    const ev = {
      data,
      source: iframe.contentWindow,
    } as unknown as MessageEvent
    for (const l of [...this.listeners]) l(ev)
  }

  dispatchFromSource(source: unknown, data: unknown): void {
    const ev = { data, source } as unknown as MessageEvent
    for (const l of [...this.listeners]) l(ev)
  }

  get mountedCount(): number {
    return this.mounted.length
  }
}

/** Minimal iframe stub — carries a unique contentWindow + srcdoc slot. */
class FakeIframe {
  public contentWindow: { postMessage: (msg: unknown, target: string) => void; __id: number }
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
      postMessage: (msg: unknown, _target: string) => {
        this.sent.push(msg)
      },
    }
    this.parentNode = {
      removeChild: () => {
        /* no-op; FakeWindow.body tracks mount state */
      },
    }
  }

  setAttribute(k: string, v: string): void {
    this.attributes.set(k, v)
  }
  getAttribute(k: string): string | undefined {
    return this.attributes.get(k)
  }
}

// ─── Fake PluginAPI ──────────────────────────────────────────────────────────

function makeStubApi(): PluginAPI {
  const throwing = (n: string) => () => {
    throw new Error(`unmocked: ${n}`)
  }
  const commandsMap = new Map<
    string,
    (...args: unknown[]) => unknown
  >()
  return {
    commands: {
      register: (id: string, handler: (...args: unknown[]) => unknown) => {
        commandsMap.set(id, handler)
      },
      execute: async (id: string, ...args: unknown[]) => {
        const h = commandsMap.get(id)
        if (!h) return undefined
        return await h(...args)
      },
      all: () => [...commandsMap.keys()].map((id) => ({ id, title: id })),
      unregister: (id: string) => {
        commandsMap.delete(id)
      },
    } as unknown as PluginAPI['commands'],
    views: { register: throwing('views.register') },
    workspace: {} as PluginAPI['workspace'],
    viewRegistry: {} as PluginAPI['viewRegistry'],
    context: {
      set: throwing('context.set'),
      get: throwing('context.get'),
      evaluate: throwing('context.evaluate'),
    },
    events: { on: throwing('events.on'), emit: throwing('events.emit') },
    storage: {
      get: throwing('storage.get'),
      set: throwing('storage.set'),
      delete: throwing('storage.delete'),
      clear: throwing('storage.clear'),
    },
    statusBar: { createItem: throwing('statusBar.createItem') },
    configuration: {
      register: throwing('configuration.register'),
      getValue: throwing('configuration.getValue'),
      setValue: throwing('configuration.setValue'),
      onChange: throwing('configuration.onChange'),
    },
    notifications: { show: throwing('notifications.show') },
    fs: {
      read: throwing('fs.read'),
      write: throwing('fs.write'),
      list: throwing('fs.list'),
      watch: throwing('fs.watch'),
      exists: throwing('fs.exists'),
      mkdir: throwing('fs.mkdir'),
      delete: throwing('fs.delete'),
      rename: throwing('fs.rename'),
    },
    kernel: {
      invoke: throwing('kernel.invoke'),
      on: throwing('kernel.on'),
      available: throwing('kernel.available'),
    },
    platform: {
      fs: {
        readText: throwing('platform.fs.readText'),
        writeText: throwing('platform.fs.writeText'),
        readDir: throwing('platform.fs.readDir'),
        exists: throwing('platform.fs.exists'),
        mkdir: throwing('platform.fs.mkdir'),
        remove: throwing('platform.fs.remove'),
        rename: throwing('platform.fs.rename'),
      },
      dialog: {} as PluginAPI['platform']['dialog'],
      window: {} as PluginAPI['platform']['window'],
      shell: {} as PluginAPI['platform']['shell'],
    },
    activityBar: {
      addItem: throwing('activityBar.addItem'),
      removeItem: throwing('activityBar.removeItem'),
    },
    input: { prompt: throwing('input.prompt'), confirm: throwing('input.confirm') },
    uri: { register: throwing('uri.register') },
  } as unknown as PluginAPI
}

function makeStubRegistry(): {
  commands: { unregister: (id: string) => void }
  trackSubscription: () => void
} {
  return {
    commands: { unregister: () => {} },
    trackSubscription: () => {},
  }
}

async function tick(n = 1): Promise<void> {
  for (let i = 0; i < n; i++) await Promise.resolve()
}

// ─── IframePort tests ────────────────────────────────────────────────────────

test('IframePort delivers envelopes arriving from its iframe source', () => {
  const win = new FakeWindow()
  const iframe = new FakeIframe()
  const port = new IframePort({ iframe, window: win })
  const received: RpcEnvelope[] = []
  port.onmessage = (ev) => received.push(ev.data as RpcEnvelope)

  const envelope = makeHandshakeAccept({
    protocolVersion: SANDBOX_PROTOCOL_VERSION,
    pluginInstanceId: 'p#1',
    methods: [],
    nonce: 'n-1',
  })
  win.dispatchFromIframe(iframe, envelope)
  assert.equal(received.length, 1)
  assert.equal(received[0].id, 'n-1')
  port.close()
})

test('IframePort drops frames from other sources (identity guard)', () => {
  const win = new FakeWindow()
  const iframe = new FakeIframe()
  const port = new IframePort({ iframe, window: win })
  const received: unknown[] = []
  port.onmessage = (ev) => received.push(ev.data)

  const evil = { __id: 999, postMessage: () => {} }
  win.dispatchFromSource(
    evil,
    makeEvent('sub-x', 'events.on', { forged: true }),
  )
  assert.equal(received.length, 0)
  port.close()
})

test('IframePort drops non-envelope data', () => {
  const win = new FakeWindow()
  const iframe = new FakeIframe()
  const port = new IframePort({ iframe, window: win })
  const received: unknown[] = []
  port.onmessage = (ev) => received.push(ev.data)

  win.dispatchFromIframe(iframe, { not: 'an envelope' })
  win.dispatchFromIframe(iframe, 'plain string')
  win.dispatchFromIframe(iframe, 42)
  assert.equal(received.length, 0)
  port.close()
})

test('IframePort.postMessage forwards to iframe.contentWindow with "*"', () => {
  const win = new FakeWindow()
  const iframe = new FakeIframe()
  const port = new IframePort({ iframe, window: win })
  port.postMessage({ ping: 1 })
  assert.equal(iframe.sent.length, 1)
  assert.deepEqual(iframe.sent[0], { ping: 1 })
  port.close()
})

test('IframePort.close detaches the listener and is idempotent', () => {
  const win = new FakeWindow()
  const iframe = new FakeIframe()
  const port = new IframePort({ iframe, window: win })
  const received: unknown[] = []
  port.onmessage = (ev) => received.push(ev.data)

  port.close()
  // Second close must be a no-op.
  port.close()
  win.dispatchFromIframe(
    iframe,
    makeEvent('sub-x', 'events.on', {}),
  )
  assert.equal(received.length, 0)
})

// ─── Orchestrator tests ──────────────────────────────────────────────────────

/**
 * Test harness: intercepts the router's handshake-accept frame before
 * it reaches the iframe and instead delivers a test-controlled
 * handshake response. This lets us drive both the happy path and the
 * reject path without a real guest bundle.
 */
function makeOrchestrator(
  overrides: Partial<SandboxOrchestratorOptions> = {},
): {
  win: FakeWindow
  orch: SandboxOrchestrator
} {
  const win = new FakeWindow()
  const api = makeStubApi()
  const registry = makeStubRegistry() as unknown as SandboxOrchestratorOptions['registry']
  const orch = new SandboxOrchestrator({
    api,
    registry,
    window: win as unknown as Window & typeof globalThis,
    handshakeTimeoutMs: 100,
    pingIntervalMs: 50,
    maxMissedPongs: 2,
    warn: () => {},
    ...overrides,
  })
  return { win, orch }
}

/**
 * Drive a successful handshake on behalf of the guest by synthesizing
 * the host-side accept frame and dispatching it from the iframe's
 * contentWindow so the orchestrator's listener settles.
 */
function acceptHandshake(win: FakeWindow): void {
  // Reach into the FakeWindow's private `mounted` list (populated via
  // `document.body.appendChild`) to find iframes attached during this
  // test. Only one is in flight per test; we fan out the accept to
  // every iframe rather than tracking the "most recent" one, because
  // the disposeAll test mounts two sequentially.
  const w = win as unknown as {
    mounted?: unknown[]
    dispatchFromIframe: (i: FakeIframe, d: unknown) => void
  }
  const mounted = (w.mounted ?? []) as FakeIframe[]
  for (const i of mounted) {
    w.dispatchFromIframe(
      i,
      makeHandshakeAccept({
        protocolVersion: SANDBOX_PROTOCOL_VERSION,
        pluginInstanceId: 'test.plugin#1',
        methods: [],
        nonce: 'hs-1',
      }),
    )
  }
}

function rejectHandshake(win: FakeWindow): void {
  const w = win as unknown as {
    mounted?: unknown[]
    dispatchFromIframe: (i: FakeIframe, d: unknown) => void
  }
  for (const i of (w.mounted ?? []) as FakeIframe[]) {
    w.dispatchFromIframe(
      i,
      makeHandshakeReject({
        nonce: 'hs-1',
        reason: 'protocol_mismatch',
        message: 'test: reject',
      }),
    )
  }
}

test('SandboxOrchestrator.load() mounts iframe and completes handshake', async () => {
  const { win, orch } = makeOrchestrator()
  const loadPromise = orch.load({
    pluginId: 'test.plugin',
    bundleUrl: 'blob:fake-bundle',
    runtimeUrl: 'blob:fake-runtime',
    capabilities: new Set(),
  })
  // Let the orchestrator mount + attach listeners.
  await tick(2)
  assert.equal(win.mountedCount, 1, 'iframe should be mounted')
  acceptHandshake(win)
  const instance = await loadPromise
  assert.equal(instance.pluginId, 'test.plugin')
  assert.equal(instance.state, 'active')
  await instance.dispose()
})

test('SandboxOrchestrator.load() rejects on handshake timeout', async () => {
  const { orch } = makeOrchestrator({ handshakeTimeoutMs: 20 })
  await assert.rejects(
    () =>
      orch.load({
        pluginId: 'slow.plugin',
        bundleUrl: 'blob:fake-bundle',
        runtimeUrl: 'blob:fake-runtime',
        capabilities: new Set(),
      }),
    /handshake timeout/,
  )
})

test('SandboxOrchestrator.load() rejects on guest-sent handshake reject', async () => {
  const { win, orch } = makeOrchestrator()
  const loadPromise = orch.load({
    pluginId: 'bad.plugin',
    bundleUrl: 'blob:fake-bundle',
    runtimeUrl: 'blob:fake-runtime',
    capabilities: new Set(),
  })
  await tick(2)
  rejectHandshake(win)
  await assert.rejects(loadPromise, /rejected handshake/)
})

test('SandboxOrchestrator watchdog crashes instance after missed pongs', async () => {
  const { win, orch } = makeOrchestrator({
    pingIntervalMs: 15,
    maxMissedPongs: 2,
  })
  const loadPromise = orch.load({
    pluginId: 'wd.plugin',
    bundleUrl: 'blob:b',
    runtimeUrl: 'blob:r',
    capabilities: new Set(),
  })
  await tick(2)
  acceptHandshake(win)
  const instance = await loadPromise
  assert.equal(instance.state, 'active')
  // Wait for two ping intervals with no pong — the watchdog should
  // transition the instance to `crashed`.
  await new Promise((r) => setTimeout(r, 80))
  assert.equal(instance.state, 'crashed')
})

test('SandboxOrchestrator watchdog pong resets the missed counter', async () => {
  const { win, orch } = makeOrchestrator({
    pingIntervalMs: 15,
    maxMissedPongs: 3,
  })
  const loadPromise = orch.load({
    pluginId: 'alive.plugin',
    bundleUrl: 'blob:b',
    runtimeUrl: 'blob:r',
    capabilities: new Set(),
  })
  await tick(2)
  acceptHandshake(win)
  const instance = await loadPromise
  // Keep delivering pongs faster than the watchdog can time out.
  const iframe = (
    win as unknown as { mounted: FakeIframe[] }
  ).mounted[0]!
  const pongTimer = setInterval(() => {
    win.dispatchFromIframe(
      iframe,
      makeEvent('pong', 'sandbox.pong', { ts: Date.now() }),
    )
  }, 5)
  await new Promise((r) => setTimeout(r, 80))
  clearInterval(pongTimer)
  assert.equal(
    instance.state,
    'active',
    'pongs should keep the instance alive',
  )
  await instance.dispose()
})

test('SandboxOrchestrator.unload tears down iframe and router', async () => {
  const { win, orch } = makeOrchestrator()
  const loadPromise = orch.load({
    pluginId: 'gone.plugin',
    bundleUrl: 'blob:b',
    runtimeUrl: 'blob:r',
    capabilities: new Set(),
  })
  await tick(2)
  acceptHandshake(win)
  const instance = await loadPromise
  assert.equal(win.mountedCount, 1)
  await orch.unload('gone.plugin')
  assert.equal(instance.state, 'disposed')
  assert.equal(orch.get('gone.plugin'), undefined)
})

test('SandboxOrchestrator.disposeAll tears down every instance', async () => {
  const { win, orch } = makeOrchestrator()
  const p1 = orch.load({
    pluginId: 'a.plugin',
    bundleUrl: 'blob:b',
    runtimeUrl: 'blob:r',
    capabilities: new Set(),
  })
  await tick(2)
  acceptHandshake(win)
  const i1 = await p1

  const p2 = orch.load({
    pluginId: 'b.plugin',
    bundleUrl: 'blob:b',
    runtimeUrl: 'blob:r',
    capabilities: new Set(),
  })
  await tick(2)
  acceptHandshake(win)
  const i2 = await p2

  await orch.disposeAll()
  assert.equal(i1.state, 'disposed')
  assert.equal(i2.state, 'disposed')
})

test('SandboxOrchestrator.load rejects duplicate pluginId', async () => {
  const { win, orch } = makeOrchestrator()
  const p1 = orch.load({
    pluginId: 'dup.plugin',
    bundleUrl: 'blob:b',
    runtimeUrl: 'blob:r',
    capabilities: new Set(),
  })
  await tick(2)
  acceptHandshake(win)
  const i1 = await p1
  await assert.rejects(
    orch.load({
      pluginId: 'dup.plugin',
      bundleUrl: 'blob:b',
      runtimeUrl: 'blob:r',
      capabilities: new Set(),
    }),
    /already loaded/,
  )
  await i1.dispose()
})

// ─── PanelNode renderer tests ────────────────────────────────────────────────

test('renderPanelNode emits nested React elements for a vstack tree', () => {
  const tree: PanelNode = {
    type: 'vstack',
    gap: 4,
    children: [
      { type: 'heading', value: 'Hello', level: 2 },
      { type: 'text', value: 'world', muted: true },
      { type: 'spacer', size: 12 },
      { type: 'button', label: 'Go', commandId: 'test.run' },
    ],
  }
  const api = {
    commands: {
      register: () => {},
      execute: async () => undefined,
      all: () => [],
    } as unknown as PluginAPI['commands'],
  }
  const element = renderPanelNode(tree, api) as unknown as {
    type: string
    props: { children: unknown[] }
  }
  assert.equal(element.type, 'div')
  assert.equal(Array.isArray(element.props.children), true)
  assert.equal(element.props.children.length, 4)
  const [heading, text, spacer, button] = element.props.children as Array<{
    type: string | ((p: unknown) => unknown)
    props: Record<string, unknown>
  }>
  assert.equal(heading.type, 'h2')
  assert.equal(heading.props.children, 'Hello')
  assert.equal(text.type, 'span')
  assert.equal(spacer.type, 'div')
  assert.equal(button.type, 'button')
  assert.equal(button.props.children, 'Go')
})

test('renderPanelNode button onClick calls api.commands.execute', async () => {
  const calls: string[] = []
  const api = {
    commands: {
      register: () => {},
      execute: async (id: string) => {
        calls.push(id)
        return undefined
      },
      all: () => [],
    } as unknown as PluginAPI['commands'],
  }
  const tree: PanelNode = {
    type: 'button',
    label: 'Fire',
    commandId: 'my.cmd',
  }
  const element = renderPanelNode(tree, api) as unknown as {
    props: { onClick: () => void }
  }
  element.props.onClick()
  await tick(2)
  assert.deepEqual(calls, ['my.cmd'])
})

// ─── Helpers ─────────────────────────────────────────────────────────────────

test('buildSandboxSrcDoc embeds runtime + bundle URLs and CSP', () => {
  const doc = buildSandboxSrcDoc({
    runtimeUrl: 'blob:http://x/runtime',
    bundleUrl: 'blob:http://x/bundle',
  })
  assert.match(doc, /runtime/)
  assert.match(doc, /bundle/)
  assert.match(doc, /Content-Security-Policy/)
  assert.match(doc, /bootstrapSandboxedPlugin/)
  // Ensure JSON.stringify was used — URLs should appear quoted.
  assert.match(doc, /"blob:http:\/\/x\/runtime"/)
})

test('SandboxInstance type shape exposes expected fields', async () => {
  const { win, orch } = makeOrchestrator()
  const p = orch.load({
    pluginId: 'shape.plugin',
    bundleUrl: 'blob:b',
    runtimeUrl: 'blob:r',
    capabilities: new Set(),
  })
  await tick(2)
  acceptHandshake(win)
  const instance: SandboxInstance = await p
  assert.equal(typeof instance.pluginId, 'string')
  assert.equal(typeof instance.dispose, 'function')
  assert.equal(typeof instance.renderPanel, 'function')
  assert.ok(instance.router)
  assert.ok(instance.iframe)
  await instance.dispose()
})
