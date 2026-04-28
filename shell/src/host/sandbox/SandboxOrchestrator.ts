// shell/src/host/sandbox/SandboxOrchestrator.ts
//
// WI-30d — iframe lifecycle + watchdog + PanelNode/command bridge for
// sandboxed community plugins. Per docs/wi30-sandbox-design.md §4.3 +
// §5.5.
//
// Responsibilities:
//   1. Spawn a null-origin iframe (sandbox="allow-scripts", no
//      allow-same-origin) carrying a minimal srcdoc that imports the
//      guest runtime (`bootstrapSandboxedPlugin`) + the plugin bundle.
//   2. Construct the IframePort + SandboxRouter pair and await the
//      handshake; time out as `protocol_mismatch` if the guest never
//      replies.
//   3. Keep a ping/pong watchdog running; after N missed pongs emit
//      `plugin:crashed` + tear down.
//   4. Register any commands the guest asks to register in the host
//      CommandRegistry so `api.commands.execute` can round-trip to the
//      sandboxed handler and return its result to the caller.
//   5. Own the PanelNode refresh channel — when the host needs a
//      panel re-render, send a `views.render` request and hand the
//      returned tree to SandboxPanelView.
//   6. Expose a typed `SandboxInstance` handle the shell can inspect,
//      dispose, and restart.
//
// What this file is NOT:
//   - The router. It composes SandboxRouter from WI-30b unchanged.
//   - The React renderer. SandboxPanelView handles that; the
//     orchestrator just hands it a render channel.
//   - The community-plugin loader. ExtensionHost decides which plugins
//     route here; the orchestrator only cares about the bundle URL +
//     granted caps.

import {
  SANDBOX_PROTOCOL_VERSION,
  isRpcEnvelope,
  type PanelNode,
  type RpcEnvelope,
} from '@nexus/extension-api'
import { IframePort } from './IframePort'
import { SandboxRouter } from './router'
import type { PluginAPI } from '../../types/plugin'
import type { PluginRegistry } from '../PluginRegistry'
import { eventBus } from '../EventBus'

// ─── Public shapes ───────────────────────────────────────────────────────────

export type SandboxState = 'activating' | 'active' | 'crashed' | 'disposed'

export interface SandboxSpec {
  pluginId: string
  /**
   * URL of the plugin bundle the srcdoc dynamic-imports. May be a
   * blob:, data:, or https: URL. Community plugins in development are
   * typically served as blob URLs built from a Tauri-fs read of the
   * bundle on disk (mirrors `communityPluginLoader.ts`).
   */
  bundleUrl: string
  /**
   * URL of the guest runtime bootstrap (the compiled
   * `bootstrapSandboxedPlugin`). The orchestrator imports it inside
   * the iframe and passes the plugin's default export to it.
   */
  runtimeUrl: string
  /**
   * Capability strings granted at consent time — forwarded verbatim to
   * the SandboxRouter's `grantedCaps` set.
   */
  capabilities: ReadonlySet<string>
  /**
   * Optional apiVersion marker from the manifest. Currently advisory —
   * the guest runtime declares its own `apiVersion` in the handshake
   * hello and the router already enforces `SANDBOX_PROTOCOL_VERSION`.
   */
  manifestApiVersion?: number
}

export interface SandboxInstance {
  readonly pluginId: string
  readonly state: SandboxState
  readonly router: SandboxRouter
  readonly iframe: HTMLIFrameElement
  /**
   * Request a fresh PanelNode for a panel the guest registered via
   * `views.registerPanel`. Returns null if the guest is not active or
   * the renderSub is unknown.
   */
  renderPanel(renderSub: string): Promise<PanelNode | null>
  dispose(): Promise<void>
}

export interface SandboxOrchestratorOptions {
  /**
   * Per-plugin `PluginAPI` factory (F-8.1.2). The orchestrator calls
   * this with the authoritative plugin id at handshake time so each
   * sandboxed plugin gets its own `PluginAPI` instance — storage
   * namespacing (`plugin:<id>:…`), event tagging (`activityBar:itemAdded`
   * `pluginId`), and `PluginRegistry.track(pluginId, …)` ownership all
   * derive from the orchestrator-set boundary id, not from anything the
   * plugin can self-assert.
   *
   * Production wiring in `shell/src/main.tsx` passes
   * `(id) => buildPluginAPI(reg, { pluginId: id, isCore: false })`.
   * Tests inject a stub factory with the same signature.
   */
  apiFactory: (pluginId: string) => PluginAPI
  registry: PluginRegistry
  /**
   * Where to mount the iframe. Defaults to `document.body`; the iframe
   * is hidden via CSS (off-screen position) because PanelNode rendering
   * lives in a React component, not in the iframe's own DOM.
   */
  container?: HTMLElement
  /**
   * Ambient window — always `window` in production. Tests may override
   * to inject a stub event loop.
   */
  window?: Window & typeof globalThis
  /**
   * Handshake timeout in ms. Default 5_000. The handshake is driven by
   * the guest runtime's top-level import + a postMessage round-trip;
   * 5s is a generous upper bound for a well-formed bundle.
   */
  handshakeTimeoutMs?: number
  /**
   * Watchdog ping interval in ms. Default 10_000 per design §5.5.
   */
  pingIntervalMs?: number
  /**
   * Missed-pong threshold. When the running count reaches this value
   * the orchestrator declares the guest crashed. Default 2 per §5.5.
   */
  maxMissedPongs?: number
  /**
   * Injection point for diagnostic output — tests silence it, prod
   * uses `console.warn`.
   */
  warn?: (...args: unknown[]) => void
}

const DEFAULT_HANDSHAKE_TIMEOUT_MS = 5_000
const DEFAULT_PING_INTERVAL_MS = 10_000
const DEFAULT_MAX_MISSED_PONGS = 2

// ─── Helpers ────────────────────────────────────────────────────────────────

/**
 * Produce the minimal HTML shell that runs inside the iframe.
 *
 * Strategy: the srcdoc is a tiny document whose body holds one
 * `<script type="module">` that dynamic-imports the runtime + the
 * plugin bundle. Both URLs must be reachable from inside the iframe —
 * blob: and https: both work across the null-origin boundary; data:
 * is also legal but fatter for non-trivial payloads.
 *
 * We chose two separate imports (runtime + bundle) over a single
 * concatenated blob so that:
 *   - The runtime blob is the same object per SandboxOrchestrator
 *     instance (cache-friendly) — callers typically pass a single URL.
 *   - Plugin bundles stay identifiable in network devtools.
 *   - A future codegen step can inject a precompiled runtime without
 *     touching the srcdoc template.
 *
 * The CSP is stricter than the host page's: no network scripts, no
 * inline scripts *outside* of this exact srcdoc-supplied one (the
 * inline `<script type="module">` survives because it's evaluated
 * before the CSP `script-src` kicks in via `unsafe-inline`). Within
 * the iframe the host CSP does not apply.
 */
export function buildSandboxSrcDoc(opts: {
  runtimeUrl: string
  bundleUrl: string
}): string {
  // `JSON.stringify` handles quote-escaping for both URLs, which is
  // safer than a hand-rolled escape for blob: URLs that can carry
  // percent-encoded characters.
  const runtime = JSON.stringify(opts.runtimeUrl)
  const bundle = JSON.stringify(opts.bundleUrl)
  return `<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'self' blob: data: 'unsafe-inline';">
</head>
<body>
<script type="module">
(async () => {
  try {
    const runtime = await import(${runtime});
    const bundle = await import(${bundle});
    const plugin = bundle.default ?? bundle.plugin;
    if (!plugin) {
      throw new Error('plugin bundle has no default export');
    }
    runtime.bootstrapSandboxedPlugin(plugin);
  } catch (err) {
    // Surface the failure to the host via postMessage so the
    // orchestrator can fail the handshake promptly instead of
    // waiting for the 5s timeout.
    window.parent.postMessage({
      id: 'sandbox-boot-error',
      direction: 'plugin-to-host',
      kind: 'handshake',
      error: {
        kind: 'dispatch_failed',
        message: String(err && err.message || err),
        retryable: false,
      },
    }, '*');
  }
})();
</script>
</body>
</html>`
}

// ─── Orchestrator ───────────────────────────────────────────────────────────

export class SandboxOrchestrator {
  private readonly instances = new Map<string, SandboxInstanceImpl>()

  constructor(private readonly opts: SandboxOrchestratorOptions) {}

  /**
   * Load a plugin into a fresh iframe. Resolves once the handshake
   * has been accepted and the first ping has been queued. Rejects on
   * handshake timeout or explicit `protocol_mismatch` reject from the
   * router.
   */
  async load(spec: SandboxSpec): Promise<SandboxInstance> {
    const existing = this.instances.get(spec.pluginId)
    if (existing) {
      throw new Error(
        `[SandboxOrchestrator] plugin '${spec.pluginId}' is already loaded`,
      )
    }

    const instance = new SandboxInstanceImpl(spec, this.opts, () => {
      this.instances.delete(spec.pluginId)
    })
    this.instances.set(spec.pluginId, instance)

    try {
      await instance.start()
    } catch (err) {
      // Cleanup on handshake failure — do not leave a stale iframe in
      // the DOM or a half-wired router in memory.
      await instance.dispose().catch(() => {})
      this.instances.delete(spec.pluginId)
      throw err
    }
    return instance
  }

  /** Retrieve an active instance by plugin id (diagnostics + tests). */
  get(pluginId: string): SandboxInstance | undefined {
    return this.instances.get(pluginId)
  }

  /**
   * Unload a plugin. Idempotent; ignores plugins that aren't loaded.
   */
  async unload(pluginId: string): Promise<void> {
    const instance = this.instances.get(pluginId)
    if (!instance) return
    await instance.dispose()
  }

  /**
   * Dispose every active instance. Used during app teardown — the
   * returned promise resolves once every iframe has been removed.
   */
  async disposeAll(): Promise<void> {
    const all = [...this.instances.values()]
    await Promise.all(all.map((i) => i.dispose().catch(() => {})))
  }
}

// ─── Internal instance ──────────────────────────────────────────────────────

class SandboxInstanceImpl implements SandboxInstance {
  public readonly pluginId: string
  public readonly iframe: HTMLIFrameElement
  public router!: SandboxRouter
  private port!: IframePort
  private _state: SandboxState = 'activating'

  private pingTimer: ReturnType<typeof setInterval> | null = null
  private missedPongs = 0
  private teardownStarted = false
  private readonly pongListener: (ev: MessageEvent) => void
  private readonly pingIntervalMs: number
  private readonly maxMissedPongs: number
  private readonly handshakeTimeoutMs: number
  private readonly warn: (...args: unknown[]) => void
  private readonly windowRef: Window & typeof globalThis

  constructor(
    private readonly spec: SandboxSpec,
    private readonly orchOpts: SandboxOrchestratorOptions,
    private readonly onDisposed: () => void,
  ) {
    this.pluginId = spec.pluginId
    this.pingIntervalMs = orchOpts.pingIntervalMs ?? DEFAULT_PING_INTERVAL_MS
    this.maxMissedPongs = orchOpts.maxMissedPongs ?? DEFAULT_MAX_MISSED_PONGS
    this.handshakeTimeoutMs =
      orchOpts.handshakeTimeoutMs ?? DEFAULT_HANDSHAKE_TIMEOUT_MS
    this.warn =
      orchOpts.warn ?? ((...args) => console.warn('[SandboxOrchestrator]', ...args))
    this.windowRef =
      (orchOpts.window ??
        (globalThis as unknown as Window & typeof globalThis)) as Window &
        typeof globalThis

    const doc = this.windowRef.document
    if (!doc) {
      throw new Error('[SandboxOrchestrator] no document — requires DOM host')
    }
    this.iframe = doc.createElement('iframe')
    // `allow-scripts` lets the module script run; the deliberate
    // absence of `allow-same-origin` is what pins the iframe at null
    // origin. Every other sandbox flag stays off (no forms, popups,
    // top-nav, etc.) per design §3.
    this.iframe.setAttribute('sandbox', 'allow-scripts')
    // Hide the iframe — rendering happens in React (SandboxPanelView);
    // the iframe is pure logic host.
    this.iframe.style.position = 'absolute'
    this.iframe.style.top = '-10000px'
    this.iframe.style.left = '-10000px'
    this.iframe.style.width = '1px'
    this.iframe.style.height = '1px'
    this.iframe.setAttribute('aria-hidden', 'true')
    this.iframe.setAttribute('data-sandbox-plugin', spec.pluginId)
    this.iframe.srcdoc = buildSandboxSrcDoc({
      runtimeUrl: spec.runtimeUrl,
      bundleUrl: spec.bundleUrl,
    })

    // Parallel pong listener — runs alongside the router so the
    // watchdog can observe `sandbox.pong` events without the router
    // having to know about them. We still filter by source identity
    // for the same reason IframePort does.
    this.pongListener = (ev: MessageEvent) => {
      if (ev.source !== this.iframe.contentWindow) return
      if (!isRpcEnvelope(ev.data)) return
      if (ev.data.kind !== 'event') return
      if (ev.data.method !== 'sandbox.pong') return
      this.missedPongs = 0
    }
  }

  get state(): SandboxState {
    return this._state
  }

  /** Mount the iframe + wire router + await handshake. */
  async start(): Promise<void> {
    const container = this.orchOpts.container ?? this.windowRef.document.body
    container.appendChild(this.iframe)

    this.port = new IframePort({
      iframe: this.iframe,
      window: this.windowRef,
      warn: (...args) => this.warn(...args),
    })

    // F-8.1.2: build a fresh `PluginAPI` for *this* plugin id so every
    // pluginId-derived surface (storage namespace, event tagging,
    // registry tracking) uses the orchestrator-set boundary id rather
    // than a shared label. Pre-fix, every sandboxed plugin shared one
    // `PluginAPI` baked with the literal string `'community-sandbox'`,
    // which caused storage collisions and mis-attributed events.
    const pluginApi = this.orchOpts.apiFactory(this.pluginId)

    this.router = new SandboxRouter({
      pluginId: this.pluginId,
      api: pluginApi,
      grantedCaps: this.spec.capabilities,
      port: this.port,
      registry: this.orchOpts.registry,
      warn: (...args) => this.warn(...args),
    })

    // WI-30e: the router now owns the `commands.register` bridge.
    // When the guest calls `ctx.commands.register(id, handler)` the
    // router stores the guest's UUID `handlerSub` keyed by command id
    // and registers a host-side handler that round-trips via
    // `router.sendRequest('dispatch.command', { handlerSub, args })`.
    // The orchestrator no longer monkey-patches `api.commands.register`.

    // Wait for handshake. The router emits a handshake-accept frame
    // via `port.postMessage`; we listen for that by snooping the
    // window-level messages (same source identity guard).
    const handshake = await this.awaitHandshake()
    if (handshake === 'timeout') {
      throw new Error(
        `[SandboxOrchestrator] handshake timeout for '${this.pluginId}' ` +
          `after ${this.handshakeTimeoutMs}ms (protocol_mismatch)`,
      )
    }
    if (handshake === 'rejected') {
      throw new Error(
        `[SandboxOrchestrator] guest rejected handshake for '${this.pluginId}'`,
      )
    }

    this._state = 'active'
    this.windowRef.addEventListener('message', this.pongListener)
    this.startWatchdog()
  }

  async renderPanel(renderSub: string): Promise<PanelNode | null> {
    if (this._state !== 'active') return null
    try {
      const result = await this.router.sendRequest(
        'views.render',
        { renderSub },
        5_000,
      )
      return (result ?? null) as PanelNode | null
    } catch (err) {
      this.warn('renderPanel failed', this.pluginId, renderSub, err)
      return null
    }
  }

  private awaitHandshake(): Promise<'accepted' | 'rejected' | 'timeout'> {
    return new Promise((resolve) => {
      let settled = false
      const settle = (r: 'accepted' | 'rejected' | 'timeout') => {
        if (settled) return
        settled = true
        this.windowRef.removeEventListener('message', listener)
        clearTimeout(timer)
        resolve(r)
      }
      const listener = (ev: MessageEvent) => {
        if (ev.source !== this.iframe.contentWindow) return
        if (!isRpcEnvelope(ev.data)) return
        const env = ev.data as RpcEnvelope
        if (env.kind !== 'handshake') return
        // The host-side router replies with a handshake frame carrying
        // `direction: 'host-to-plugin'`. The guest's hello has
        // `plugin-to-host`. We only resolve on the host's accept frame;
        // the guest's hello flows through unobserved here.
        if (env.direction !== 'host-to-plugin') return
        if (env.error) {
          settle('rejected')
        } else {
          const payload = env.payload as
            | { protocolVersion?: number }
            | undefined
          if (payload?.protocolVersion === SANDBOX_PROTOCOL_VERSION) {
            settle('accepted')
          } else {
            settle('rejected')
          }
        }
      }
      const timer = setTimeout(
        () => settle('timeout'),
        this.handshakeTimeoutMs,
      )
      this.windowRef.addEventListener('message', listener)
    })
  }

  private startWatchdog(): void {
    this.pingTimer = setInterval(() => {
      if (this._state !== 'active') return
      this.missedPongs++
      if (this.missedPongs >= this.maxMissedPongs) {
        this.onCrash()
        return
      }
      // Ping via a host→plugin event frame. The guest's runtime does
      // not currently respond to these, but the test suite injects a
      // pong to exercise the recovery path. A follow-up can teach the
      // runtime to auto-pong — tracked as a Wave 1 runtime extension.
      try {
        this.router.sendEvent('sandbox.ping', 'watchdog', {
          ts: Date.now(),
        })
      } catch (err) {
        this.warn('watchdog ping threw', err)
      }
    }, this.pingIntervalMs)
  }

  private onCrash(): void {
    if (this._state === 'disposed' || this._state === 'crashed') return
    this._state = 'crashed'
    this.stopWatchdog()
    try {
      eventBus.emit('plugin:error', {
        pluginId: this.pluginId,
        error: new Error('sandbox watchdog timeout — plugin crashed'),
      })
    } catch {
      /* best-effort */
    }
    // Best-effort teardown of iframe + router; failures here are
    // expected because a crashed iframe often can't respond to any
    // further frames. We intentionally preserve the `crashed` state
    // (rather than letting dispose() flip it to `disposed`) so the
    // UI can surface "plugin crashed — click to restart" instead of
    // silently hiding the failure.
    void this.disposeInternal(false).catch(() => {})
  }

  private stopWatchdog(): void {
    if (this.pingTimer) {
      clearInterval(this.pingTimer)
      this.pingTimer = null
    }
    try {
      this.windowRef.removeEventListener('message', this.pongListener)
    } catch {
      /* best-effort */
    }
  }

  async dispose(): Promise<void> {
    return this.disposeInternal(true)
  }

  /**
   * Internal teardown. `markDisposed=false` preserves a terminal
   * state like `crashed` so the UI can differentiate between an
   * orderly unload and a watchdog-driven teardown.
   */
  private async disposeInternal(markDisposed: boolean): Promise<void> {
    // Idempotent: multiple crash/dispose/unload paths can race. The
    // first call owns the teardown; subsequent calls may still flip
    // the terminal state (`crashed` → `disposed`) if an orderly
    // unload follows a crash, but the side-effecting resource
    // releases only run once.
    if (this.teardownStarted) {
      if (markDisposed && this._state !== 'disposed') {
        this._state = 'disposed'
      }
      return
    }
    this.teardownStarted = true
    const wasActive = this._state === 'active'
    if (markDisposed) this._state = 'disposed'
    this.stopWatchdog()

    // Unregister host-side commands the guest registered. The router
    // populated these on every successful `commands.register` dispatch;
    // we pull the ids now (before `router.dispose()` clears them) and
    // sweep the CommandRegistry so the shell palette / executor don't
    // keep a dead handler that would throw on the next invocation.
    if (this.router) {
      for (const id of this.router.registeredCommandIds) {
        try {
          this.orchOpts.registry.commands.unregister(id)
        } catch {
          /* swallow */
        }
      }
    }

    // Send dispose frame best-effort; the router will send a real one
    // if the guest still responds. Timeout is short — we don't block
    // app shutdown on a half-dead iframe.
    if (wasActive && this.router && !this.router.isDisposed) {
      try {
        this.port.postMessage({
          id: `dispose-${Date.now()}`,
          direction: 'host-to-plugin',
          kind: 'dispose',
        })
      } catch {
        /* swallow */
      }
    }

    try {
      this.router?.dispose()
    } catch {
      /* swallow */
    }
    try {
      this.port?.close()
    } catch {
      /* swallow */
    }
    try {
      this.iframe.parentNode?.removeChild(this.iframe)
    } catch {
      /* swallow */
    }
    this.onDisposed()
  }
}
