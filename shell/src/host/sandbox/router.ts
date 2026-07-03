// shell/src/host/sandbox/router.ts
//
// WI-30b — host-side RPC router for the community-plugin sandbox.
// Per docs/wi30-sandbox-design.md §5.1-§5.6.
//
// Responsibilities:
//   1. Accept inbound `RpcEnvelope`s from the guest (handshake + request
//      + dispose) and dispatch them to the appropriate `api.*` surface.
//   2. Check capabilities up front; fail fast with
//      `capability_denied` without touching the backing surface.
//   3. Bridge the subscription pattern (§5.6) — when the guest calls
//      `kernel.on` with a `handlerSub`, store the real disposer locally
//      and push events back out as `event` envelopes correlated by the
//      guest's subscriptionId.
//   4. Enforce a per-request timeout (on the HOST side, not the guest)
//      so a misbehaving `api.*` implementation can't leave a request
//      dangling forever in tests / production.
//   5. Tear down all subscriptions on `dispose()`; subsequent dispatches
//      reject with `plugin_disposed`.
//
// What this file is NOT:
//   - The iframe wrapper. The router is given a `SandboxPort` (an
//     abstracted postMessage endpoint) at construction time; WI-30d
//     will supply an adapter around `iframe.contentWindow` + a guarded
//     message listener. Tests supply a `MessageChannel` port directly.
//   - The guest bridge. Only host-side logic lives here.
//   - The orchestrator. `SandboxOrchestrator` (WI-30a follow-on) will
//     own iframe creation + watchdog heartbeat + crash detection and
//     compose over `SandboxRouter`.

import type { PluginAPI } from '../../types/plugin'
import type { PluginRegistry } from '../PluginRegistry'
import {
  SANDBOX_PROTOCOL_VERSION,
  isRpcEnvelope,
  makeErrorResponse,
  makeEvent,
  makeHandshakeAccept,
  makeHandshakeReject,
  makeRequest,
  makeResponse,
  type HandshakeHello,
  type RpcEnvelope,
  type RpcErrorEnvelope,
} from '@nexus/extension-api'
import {
  SANDBOX_METHOD_NAMES,
  SANDBOX_REJECTED_METHODS,
  type SandboxMethodName,
} from './methodCatalog'
import { checkCapability, requiredCapabilityFor } from './capabilityGuard'
import { clientLogger } from '../clientLogger'

/**
 * Minimal postMessage endpoint. Lines up with the subset of `MessagePort`
 * the router uses, and is equally satisfiable by a real iframe's
 * `{ postMessage, addEventListener('message', ...) }` contract — so the
 * same router drives both unit tests (`MessageChannel`) and production
 * iframes (WI-30d) without adaptation.
 */
export interface SandboxPort {
  postMessage(message: unknown): void
  /** Native MessagePort uses `onmessage`; the router sets it. */
  onmessage: ((ev: MessageEvent) => void) | null
  start?(): void
  close?(): void
}

/// Plugin-id prefix reserved for core (`shell/src/plugins/core/*`,
/// `shell/src/plugins/nexus/*`) plugins on the Rust kernel side. A
/// sandboxed community plugin invoking commands in this namespace via
/// `kernel.invoke` is the documented exploit shape from issue #75 —
/// it reaches core handlers (storage write/delete, terminal
/// `create_session`, mcp `connect`, …) through the shell's
/// `Capability::ALL` invoker context, with no per-caller capability
/// gate or audit-log identity attribution.
///
/// Sandboxed plugins must use the typed surfaces exposed on
/// `PluginAPI` (`api.platform.fs.*`, `api.workspace.*`, `api.events.*`,
/// `api.commands.*`, …) for those services, which preserve caller
/// identity and route through dedicated capability checks. Direct
/// `kernel.invoke` against `com.nexus.*` is rejected at the sandbox
/// boundary with `capability_denied`.
export const CORE_PLUGIN_NAMESPACE_PREFIX = 'com.nexus.'

/** Thrown from `kernel.invoke` when the target id is reserved for
 * core plugins. Caught by `normalizeError` and surfaced as
 * `capability_denied`. */
class SandboxKernelInvokeDeniedError extends Error {
  readonly targetId: string
  readonly commandId: string
  constructor(targetId: string, commandId: string) {
    super(
      `kernel.invoke into core plugin '${targetId}.${commandId}' is not allowed from sandboxed plugins; ` +
        `use the typed PluginAPI surface (api.platform.fs.*, api.workspace.*, etc.) instead`,
    )
    this.name = 'SandboxKernelInvokeDeniedError'
    this.targetId = targetId
    this.commandId = commandId
  }
}

export interface SandboxRouterOptions {
  pluginId: string
  /**
   * Host-side authoritative API. The router calls into this for every
   * method that crosses. In production this is the object returned by
   * `buildPluginAPI`; in tests it's a partial stub with only the
   * methods under test.
   */
  api: PluginAPI
  /**
   * Set of Capability-enum-variant strings the user granted this plugin
   * at consent time. The guard short-circuits any method whose required
   * cap isn't present.
   */
  grantedCaps: ReadonlySet<string>
  /** Transport to the guest. */
  port: SandboxPort
  /**
   * Optional. Lets the router register `kernel.on` / `events.on`
   * disposers under the shell-wide subscription sweep so plugin
   * unload tears them down even if the guest never sends `dispose`.
   */
  registry?: Pick<PluginRegistry, 'trackSubscription'>
  /**
   * Default request timeout in milliseconds. Requests that don't resolve
   * in this window receive a `timeout` error response. Default 30_000
   * mirrors the kernel's `kernel_invoke` default (see
   * `PluginAPI.ts:238` — `timeoutMs ?? null` lets the kernel apply its
   * own 30s default).
   */
  defaultTimeoutMs?: number
  /**
   * Per-method timeout overrides. Omitted methods fall back to
   * `defaultTimeoutMs`. Useful for `platform.dialog.*` (user-interactive
   * — no sane timeout) and `input.*` (same reason).
   */
  timeoutOverrides?: Partial<Record<SandboxMethodName, number | null>>
  /**
   * Optional logger for diagnostic output. Defaults to `console.warn`
   * so tests can inject a spy and production runs surface issues via
   * the default shell logger.
   */
  warn?: (...args: unknown[]) => void
}

const DEFAULT_TIMEOUT_MS = 30_000

/**
 * Built-in per-method timeout overrides. `null` means "no timeout" —
 * applied to user-interactive surfaces where a modal can reasonably
 * sit open indefinitely.
 */
const BUILTIN_TIMEOUT_OVERRIDES: Partial<Record<SandboxMethodName, number | null>> = {
  'platform.dialog.openFile': null,
  'platform.dialog.openDirectory': null,
  'platform.dialog.saveFile': null,
  'input.prompt': null,
  'input.confirm': null,
}

interface TrackedSubscription {
  /** Disposer returned by the backing `api.*` surface. */
  hostDispose: () => void
  /** Guest-declared category, for diagnostic logs. */
  method: string
}

/**
 * Pending host→plugin request awaiting a response. Used by
 * `sendRequest` to correlate the guest's response envelope back to the
 * promise we handed to the host-side caller (e.g. a bridged command
 * handler invoked via `api.commands.execute`).
 */
interface PendingHostRequest {
  resolve: (value: unknown) => void
  reject: (reason: unknown) => void
  timer: ReturnType<typeof setTimeout> | null
  method: string
}

/**
 * Default timeout for host→plugin requests (WI-30e). Mirrors the
 * request-timeout window used for plugin→host requests; a sandboxed
 * handler that exceeds it is reported as a `timeout` error to the host
 * caller so `api.commands.execute(id)` doesn't dangle.
 */
const HOST_REQUEST_TIMEOUT_MS = 30_000

export class SandboxRouter {
  private readonly pluginId: string
  private readonly api: PluginAPI
  private readonly grantedCaps: ReadonlySet<string>
  private readonly port: SandboxPort
  private readonly registry?: Pick<PluginRegistry, 'trackSubscription'>
  private readonly defaultTimeoutMs: number
  private readonly timeoutOverrides: Partial<Record<SandboxMethodName, number | null>>
  private readonly warn: (...args: unknown[]) => void

  private readonly subscriptions = new Map<string, TrackedSubscription>()
  private readonly pendingTimers = new Map<string, ReturnType<typeof setTimeout>>()
  /**
   * Host→plugin requests that are awaiting a response. Populated by
   * `sendRequest`; drained in `handle` when a `response` envelope with
   * `direction: plugin-to-host` lands with a matching id.
   */
  private readonly pendingHostRequests = new Map<string, PendingHostRequest>()
  /**
   * Map from guest-registered command id to the guest's UUID
   * `handlerSub`. The router uses this to route `dispatch.command`
   * host→plugin requests back to the right handler in the guest's
   * `commandHandlers` map (see `runtime.ts:445`). Replaces the
   * synthetic `cmd:${pluginId}:${id}` form the orchestrator used to
   * mint before WI-30e.
   */
  private readonly commandHandlerSubs = new Map<string, string>()
  /**
   * Counter used to mint unique host→plugin request ids. Collision
   * with guest-minted ids is impossible because guest ids are UUIDs
   * and host ids are prefixed `host-req-`.
   */
  private hostRequestCounter = 0

  private disposed = false
  private handshakeComplete = false
  /** Instance id assigned on successful handshake; echoed on every frame. */
  public pluginInstanceId: string | null = null

  constructor(opts: SandboxRouterOptions) {
    this.pluginId = opts.pluginId
    this.api = opts.api
    this.grantedCaps = opts.grantedCaps
    this.port = opts.port
    this.registry = opts.registry
    this.defaultTimeoutMs = opts.defaultTimeoutMs ?? DEFAULT_TIMEOUT_MS
    this.timeoutOverrides = { ...BUILTIN_TIMEOUT_OVERRIDES, ...(opts.timeoutOverrides ?? {}) }
    this.warn = opts.warn ?? ((...args) => clientLogger.warn('[SandboxRouter]', ...args))

    this.port.onmessage = (ev) => this.onRaw(ev.data)
    this.port.start?.()
  }

  // ─── Public API ────────────────────────────────────────────────────────

  /**
   * Dispatch an inbound envelope. Kept public so the orchestrator can
   * feed it frames from a global message listener (production path)
   * instead of wiring `port.onmessage` directly. Tests use the port
   * wiring; both paths funnel through `handle`.
   */
  async handle(envelope: RpcEnvelope): Promise<void> {
    if (this.disposed) {
      if (envelope.kind === 'request') {
        // `sendError` normally gates on `this.disposed`; bypass here so
        // the guest gets a final error frame for an in-flight request
        // whose response would otherwise never land.
        this.port.postMessage(
          makeErrorResponse(
            envelope.id,
            {
              kind: 'plugin_disposed',
              message: 'sandbox router has been disposed',
              retryable: false,
              pluginId: this.pluginId,
              method: envelope.method,
            },
            'host-to-plugin',
            envelope.method,
          ),
        )
      }
      return
    }

    switch (envelope.kind) {
      case 'handshake':
        this.handleHandshake(envelope)
        return
      case 'request':
        await this.handleRequest(envelope)
        return
      case 'dispose':
        this.handleDispose(envelope)
        return
      case 'response':
        // Guest-originated response to a host→plugin request (WI-30e).
        // Only expected for `dispatch.command`, `dispatch.uri`, and
        // `views.render`; any response without a matching pending id
        // is still logged as unexpected (could signal a misbehaving
        // guest that fabricated a response).
        this.handleHostResponse(envelope)
        return
      case 'event':
        // Plugin shouldn't be sending these; ignore to stay robust.
        this.warn('unexpected frame from guest', envelope.kind, envelope.method)
        return
    }
  }

  /**
   * Send a host→plugin request and await the guest's response. Used by
   * the `commands.register` bridge to round-trip `dispatch.command` to
   * the guest's UUID-keyed handler, and by callers (e.g. the
   * orchestrator's `renderPanel`) that need a host-initiated RPC with
   * a return value.
   */
  sendRequest(
    method: string,
    payload: unknown,
    timeoutMs: number = HOST_REQUEST_TIMEOUT_MS,
  ): Promise<unknown> {
    if (this.disposed) {
      return Promise.reject(
        Object.assign(new Error('sandbox router disposed'), {
          kind: 'plugin_disposed',
          pluginId: this.pluginId,
          method,
        }),
      )
    }
    return new Promise((resolve, reject) => {
      this.hostRequestCounter += 1
      const id = `host-req-${this.hostRequestCounter}-${Date.now()}`
      const timer =
        timeoutMs > 0
          ? setTimeout(() => {
              const entry = this.pendingHostRequests.get(id)
              if (!entry) return
              this.pendingHostRequests.delete(id)
              entry.reject({
                kind: 'timeout',
                message: `host request ${method} exceeded ${timeoutMs}ms`,
                retryable: true,
                pluginId: this.pluginId,
                method,
              } satisfies RpcErrorEnvelope)
            }, timeoutMs)
          : null
      this.pendingHostRequests.set(id, {
        resolve,
        reject,
        timer,
        method,
      })
      this.port.postMessage(makeRequest(id, method, payload, 'host-to-plugin'))
    })
  }

  private handleHostResponse(envelope: RpcEnvelope): void {
    const entry = this.pendingHostRequests.get(envelope.id)
    if (!entry) {
      this.warn('unexpected response frame from guest', envelope.id, envelope.method)
      return
    }
    this.pendingHostRequests.delete(envelope.id)
    if (entry.timer) clearTimeout(entry.timer)
    if (envelope.error) {
      entry.reject(envelope.error)
    } else {
      entry.resolve(envelope.payload)
    }
  }

  /**
   * Push a host-originated event to the guest. Used by `kernel.on` /
   * `events.on` etc. when the real subscription fires. `subscriptionId`
   * is the guest-supplied id from the original registration call.
   */
  sendEvent(method: SandboxMethodName | string, subscriptionId: string, payload: unknown): void {
    if (this.disposed) return
    this.port.postMessage(makeEvent(subscriptionId, method, payload))
  }

  /**
   * Tear down every subscription tracked for this plugin and mark the
   * router dead. Subsequent `handle()` calls reject with
   * `plugin_disposed`. Idempotent.
   */
  dispose(): void {
    if (this.disposed) return
    this.disposed = true
    for (const [, sub] of this.subscriptions) {
      try { sub.hostDispose() } catch (err) {
        this.warn('subscription disposer threw', err)
      }
    }
    this.subscriptions.clear()
    this.commandHandlerSubs.clear()
    for (const timer of this.pendingTimers.values()) clearTimeout(timer)
    this.pendingTimers.clear()
    // Reject any in-flight host→plugin requests so the host-side
    // callers (bridged command handlers, renderPanel, etc.) unblock.
    for (const [, entry] of this.pendingHostRequests) {
      if (entry.timer) clearTimeout(entry.timer)
      entry.reject({
        kind: 'plugin_disposed',
        message: `sandbox router disposed while ${entry.method} was in flight`,
        retryable: false,
        pluginId: this.pluginId,
        method: entry.method,
      } satisfies RpcErrorEnvelope)
    }
    this.pendingHostRequests.clear()
    try { this.port.close?.() } catch { /* best-effort */ }
  }

  /** Test/diagnostic helper. */
  get subscriptionCount(): number {
    return this.subscriptions.size
  }

  get isDisposed(): boolean {
    return this.disposed
  }

  /**
   * Command ids the guest has registered through `commands.register`.
   * The orchestrator reads this on teardown to remove the host-side
   * `CommandRegistry` entries (WI-30e). Snapshot — mutating the
   * returned array is safe and does not affect the router's state.
   */
  get registeredCommandIds(): string[] {
    return [...this.commandHandlerSubs.keys()]
  }

  // ─── Internal ──────────────────────────────────────────────────────────

  private onRaw(data: unknown): void {
    if (!isRpcEnvelope(data)) {
      this.warn('dropped non-envelope frame', data)
      return
    }
    // Fire-and-forget; errors inside `handle` are converted to response
    // envelopes before they bubble, so `handle` itself never throws.
    void this.handle(data).catch((err) => {
      this.warn('handle threw unexpectedly', err)
    })
  }

  private handleHandshake(envelope: RpcEnvelope): void {
    const payload = envelope.payload as Partial<HandshakeHello> | undefined
    const nonce = envelope.id
    if (!payload || typeof payload.protocolVersion !== 'number') {
      this.port.postMessage(makeHandshakeReject({
        nonce,
        reason: 'dispatch_failed',
        message: 'handshake hello missing protocolVersion',
      }))
      return
    }
    if (payload.protocolVersion !== SANDBOX_PROTOCOL_VERSION) {
      this.port.postMessage(makeHandshakeReject({
        nonce,
        reason: 'protocol_mismatch',
        message: `unsupported protocol version: ${payload.protocolVersion} (host speaks ${SANDBOX_PROTOCOL_VERSION})`,
      }))
      return
    }
    this.handshakeComplete = true
    this.pluginInstanceId = this.pluginInstanceId ?? `${this.pluginId}#${nonce}`
    this.port.postMessage(makeHandshakeAccept({
      protocolVersion: SANDBOX_PROTOCOL_VERSION,
      pluginInstanceId: this.pluginInstanceId,
      methods: [...SANDBOX_METHOD_NAMES],
      nonce,
    }))
  }

  private async handleRequest(envelope: RpcEnvelope): Promise<void> {
    const { id, method } = envelope
    if (!method) {
      this.sendError(id, method, {
        kind: 'dispatch_failed',
        message: 'request envelope missing method',
        retryable: false,
        pluginId: this.pluginId,
      })
      return
    }

    if (!this.handshakeComplete) {
      this.sendError(id, method, {
        kind: 'dispatch_failed',
        message: 'request before handshake',
        retryable: false,
        pluginId: this.pluginId,
        method,
      })
      return
    }

    // Check against the catalog first — an unknown method is cheaper
    // to reject than a cap-denied one and gives the plugin a better
    // diagnostic. Rejected methods (views.register, workspace.*) carry
    // a hint.
    if (requiredCapabilityFor(method) === undefined) {
      const hint = SANDBOX_REJECTED_METHODS[method]
      this.sendError(id, method, {
        kind: 'unknown_method',
        message: hint ?? `unknown method: ${method}`,
        retryable: false,
        pluginId: this.pluginId,
        method,
      })
      return
    }

    // Capability check.
    const cap = checkCapability(method, this.grantedCaps)
    if (!cap.allowed) {
      this.sendError(id, method, {
        kind: 'capability_denied',
        message: `plugin ${this.pluginId} lacks capability ${cap.required} for ${method}`,
        retryable: false,
        pluginId: this.pluginId,
        method,
      })
      return
    }

    // Timeout watchdog. Some methods (dialog, input) opt out via null.
    const timeoutMs = this.resolveTimeout(method as SandboxMethodName)
    let timedOut = false
    if (timeoutMs !== null) {
      const timer = setTimeout(() => {
        timedOut = true
        this.pendingTimers.delete(id)
        this.sendError(id, method, {
          kind: 'timeout',
          message: `request ${method} exceeded ${timeoutMs}ms`,
          retryable: true,
          pluginId: this.pluginId,
          method,
        })
      }, timeoutMs)
      this.pendingTimers.set(id, timer)
    }

    try {
      const result = await this.dispatchMethod(method as SandboxMethodName, envelope.payload, id)
      if (timedOut) return
      this.clearTimer(id)
      this.port.postMessage(makeResponse(id, result, 'host-to-plugin', method))
    } catch (err) {
      if (timedOut) return
      this.clearTimer(id)
      this.sendError(id, method, this.normalizeError(err, method))
    }
  }

  private handleDispose(envelope: RpcEnvelope): void {
    const payload = envelope.payload as { subscriptionId?: string } | undefined
    const subId = payload?.subscriptionId ?? envelope.id
    const sub = this.subscriptions.get(subId)
    if (sub) {
      try { sub.hostDispose() } catch (err) {
        this.warn('dispose: disposer threw', err)
      }
      this.subscriptions.delete(subId)
    }
    // Ack only if the guest correlated the dispose with an id we can echo.
    if (envelope.id && envelope.id !== subId) {
      this.port.postMessage(makeResponse(envelope.id, { disposed: true }, 'host-to-plugin', 'dispose'))
    }
  }

  private resolveTimeout(method: SandboxMethodName): number | null {
    if (method in this.timeoutOverrides) {
      const override = this.timeoutOverrides[method]
      if (override === null) return null
      if (typeof override === 'number') return override
    }
    return this.defaultTimeoutMs
  }

  private clearTimer(id: string): void {
    const timer = this.pendingTimers.get(id)
    if (timer) {
      clearTimeout(timer)
      this.pendingTimers.delete(id)
    }
  }

  private sendError(id: string, method: string | undefined, error: RpcErrorEnvelope): void {
    if (this.disposed) return
    this.port.postMessage(makeErrorResponse(id, error, 'host-to-plugin', method))
  }

  private normalizeError(err: unknown, method: string): RpcErrorEnvelope {
    const base: RpcErrorEnvelope = {
      kind: 'dispatch_failed',
      message: 'unknown error',
      retryable: false,
      pluginId: this.pluginId,
      method,
    }
    // Sandbox-side denies of `kernel.invoke` into the core namespace
    // (issue #75) surface as `capability_denied` so guests get the
    // same shape they would for any other capability gate.
    if (err instanceof SandboxKernelInvokeDeniedError) {
      return {
        ...base,
        kind: 'capability_denied',
        message: err.message,
      }
    }
    if (err instanceof Error) {
      return { ...base, message: err.message }
    }
    if (typeof err === 'string') {
      return { ...base, message: err }
    }
    if (err && typeof err === 'object' && 'message' in err) {
      return { ...base, message: String((err as { message: unknown }).message) }
    }
    return base
  }

  // ─── Method dispatch ───────────────────────────────────────────────────
  //
  // Keeps every cross-boundary call explicit. The router deliberately
  // does not introspect `this.api` dynamically — the catalog is a fixed
  // allowlist, so a new method requires a new `case` and a new test.

  private async dispatchMethod(
    method: SandboxMethodName,
    rawArgs: unknown,
    id: string,
  ): Promise<unknown> {
    const args = (rawArgs ?? {}) as Record<string, unknown>

    switch (method) {
      // ── Commands ──────────────────────────────────────────────────
      case 'commands.register': {
        const cmdId = String(args.id)
        const handlerSub = String(args.handlerSub)
        // WI-30e: the guest's `commands.register` call carries the
        // UUID `handlerSub` it minted and stored in its own
        // `commandHandlers` map (runtime.ts:217). Record that sub
        // keyed by command id, then register a host-side handler that
        // round-trips to the guest via `dispatch.command` + the real
        // sub. This way `api.commands.execute(id)` yields the guest
        // handler's return value, which the previous `sendEvent`-based
        // bridge couldn't deliver because events carry no response.
        this.commandHandlerSubs.set(cmdId, handlerSub)
        this.api.commands.register(cmdId, async (...handlerArgs: unknown[]) => {
          const sub = this.commandHandlerSubs.get(cmdId)
          if (!sub) {
            throw new Error(
              `sandboxed plugin '${this.pluginId}' lost handlerSub for command '${cmdId}'`,
            )
          }
          return await this.sendRequest('dispatch.command', {
            handlerSub: sub,
            args: handlerArgs,
          })
        })
        this.trackSub(handlerSub, 'commands.register', () => {
          // Drop the id→sub mapping so a late-arriving command
          // execute resolves via the error path above instead of
          // stalling on a dispatch.command to a dead guest.
          this.commandHandlerSubs.delete(cmdId)
        })
        return undefined
      }
      case 'commands.execute': {
        const cmdId = String(args.id)
        const cmdArgs = Array.isArray(args.args) ? args.args : []
        return await this.api.commands.execute(cmdId, ...cmdArgs)
      }
      case 'commands.all':
        return this.api.commands.all()

      // ── Kernel ───────────────────────────────────────────────────
      case 'kernel.invoke': {
        const targetId = String(args.pluginId)
        const cmdId = String(args.commandId)
        const callArgs = args.args
        const timeoutMs = typeof args.timeoutMs === 'number' ? args.timeoutMs : undefined
        // Sandboxed community plugins reach `kernel.invoke` via the
        // shell's `Capability::ALL` invoker context on the Rust side
        // (bridge.rs:301 → context.ipc_call). That context's identity
        // is "shell", not the guest's, so per-caller capability checks
        // and audit-log attribution don't apply. Core-plugin targets
        // are the documented exploit surface (issue #75) — refuse
        // them at the sandbox boundary. Sandboxed plugins reach those
        // services through the typed `api.platform.*` / `api.workspace.*`
        // surfaces, which preserve caller identity.
        if (targetId.startsWith(CORE_PLUGIN_NAMESPACE_PREFIX)) {
          throw new SandboxKernelInvokeDeniedError(targetId, cmdId)
        }
        return await this.api.kernel.invoke(targetId, cmdId, callArgs, timeoutMs)
      }
      case 'kernel.on': {
        const topicPrefix = String(args.topicPrefix)
        const handlerSub = String(args.handlerSub)
        const hostUnsub = await this.api.kernel.on(topicPrefix, (topic, payload) => {
          this.sendEvent('kernel.on', handlerSub, { topic, payload })
        })
        this.trackSub(handlerSub, 'kernel.on', hostUnsub)
        this.registry?.trackSubscription(this.pluginId, hostUnsub)
        return { subscriptionId: handlerSub }
      }
      case 'kernel.off': {
        const subId = String(args.subscriptionId)
        this.removeSub(subId)
        return undefined
      }
      case 'kernel.available':
        return await this.api.kernel.available()

      // ── Platform: fs ─────────────────────────────────────────────
      case 'platform.fs.readText':
        return await this.api.platform.fs.readText(String(args.path))
      case 'platform.fs.writeText':
        await this.api.platform.fs.writeText(String(args.path), String(args.content))
        return undefined
      case 'platform.fs.readDir':
        return await this.api.platform.fs.readDir(String(args.path))
      case 'platform.fs.exists':
        return await this.api.platform.fs.exists(String(args.path))
      case 'platform.fs.mkdir':
        await this.api.platform.fs.mkdir(
          String(args.path),
          { recursive: args.recursive as boolean | undefined },
        )
        return undefined
      case 'platform.fs.remove':
        await this.api.platform.fs.remove(String(args.path))
        return undefined
      case 'platform.fs.rename':
        await this.api.platform.fs.rename(String(args.from), String(args.to))
        return undefined

      // ── Platform: dialog ─────────────────────────────────────────
      case 'platform.dialog.openFile':
        return await this.api.platform.dialog.openFile(args.options as Parameters<typeof this.api.platform.dialog.openFile>[0])
      case 'platform.dialog.openDirectory':
        return await this.api.platform.dialog.openDirectory(args.options as Parameters<typeof this.api.platform.dialog.openDirectory>[0])
      case 'platform.dialog.saveFile':
        return await this.api.platform.dialog.saveFile(args.options as Parameters<typeof this.api.platform.dialog.saveFile>[0])

      // ── Platform: window ─────────────────────────────────────────
      case 'platform.window.minimize':
        await this.api.platform.window.minimize()
        return undefined
      case 'platform.window.toggleMaximize':
        await this.api.platform.window.toggleMaximize()
        return undefined
      case 'platform.window.close':
        await this.api.platform.window.close()
        return undefined
      case 'platform.window.isMaximized':
        return await this.api.platform.window.isMaximized()

      // ── Platform: shell ──────────────────────────────────────────
      case 'platform.shell.openExternal':
        await this.api.platform.shell.openExternal(String(args.target))
        return undefined

      // ── Platform: net (C81) ──────────────────────────────────────
      case 'platform.net.request': {
        const headers = args.headers as Record<string, string> | undefined
        const body = typeof args.body === 'string' ? args.body : undefined
        return await this.api.platform.net.request({
          method: String(args.method),
          url: String(args.url),
          headers,
          body,
        })
      }

      // ── Events ──────────────────────────────────────────────────
      case 'events.on': {
        const ev = String(args.event)
        const handlerSub = String(args.handlerSub)
        const hostUnsub = this.api.events.on(ev, (payload) => {
          this.sendEvent('events.on', handlerSub, payload)
        })
        this.trackSub(handlerSub, 'events.on', hostUnsub)
        this.registry?.trackSubscription(this.pluginId, hostUnsub)
        return { subscriptionId: handlerSub }
      }
      case 'events.off': {
        this.removeSub(String(args.subscriptionId))
        return undefined
      }
      case 'events.emit':
        this.api.events.emit(String(args.event), args.payload)
        return undefined

      // ── Storage ─────────────────────────────────────────────────
      case 'storage.get':
        return this.api.storage.get(String(args.key))
      case 'storage.set':
        this.api.storage.set(String(args.key), String(args.value))
        return undefined
      case 'storage.delete':
        this.api.storage.delete(String(args.key))
        return undefined
      case 'storage.clear':
        this.api.storage.clear()
        return undefined

      // ── Notifications ───────────────────────────────────────────
      case 'notifications.show':
        this.api.notifications.show(args.notification as Parameters<typeof this.api.notifications.show>[0])
        return undefined

      // ── Context ─────────────────────────────────────────────────
      case 'context.set':
        this.api.context.set(String(args.key), args.value)
        return undefined
      case 'context.get':
        return this.api.context.get(String(args.key))
      case 'context.evaluate':
        return this.api.context.evaluate(String(args.expression))

      // ── Status bar ──────────────────────────────────────────────
      case 'statusBar.createItem': {
        const config = args.config as Parameters<typeof this.api.statusBar.createItem>[0]
        const handle = this.api.statusBar.createItem(config)
        // The handle is a live object with getters/setters — cannot
        // cross structured clone. Mint a subscriptionId for it and
        // expose a dispose channel; future mutators (text/content
        // updates) will be separate RPCs in WI-30c.
        const handleSub = `statusBar:${config.id}:${id}`
        this.trackSub(handleSub, 'statusBar.createItem', () => {
          try { handle.dispose() } catch (err) { this.warn('statusBar dispose threw', err) }
        })
        return { handleSub }
      }

      // ── URI ─────────────────────────────────────────────────────
      case 'uri.register': {
        const scheme = String(args.scheme)
        const handlerSub = String(args.handlerSub)
        const hostUnsub = this.api.uri.register(scheme, (url) => {
          this.sendEvent('uri.register', handlerSub, { url: url.toString() })
        })
        this.trackSub(handlerSub, 'uri.register', hostUnsub)
        return { subscriptionId: handlerSub }
      }

      // ── Views ───────────────────────────────────────────────────
      case 'views.registerPanel':
        // Host cannot render a PanelNode from a subscriptionId without
        // the PanelNode renderer wire-up — tracked as WI-30d follow-on.
        // For Phase 3c we accept the registration and no-op, surfacing
        // in the sub registry so the guest still gets a clean dispose.
        this.trackSub(String(args.renderSub), 'views.registerPanel', () => {})
        return undefined

      // ── Input ───────────────────────────────────────────────────
      case 'input.prompt':
        return await this.api.input.prompt(String(args.message), args.placeholder as string | undefined)
      case 'input.confirm':
        return await this.api.input.confirm(String(args.message))

      // ── Activity bar ────────────────────────────────────────────
      case 'activityBar.addItem':
        this.api.activityBar.addItem(args.config as Parameters<typeof this.api.activityBar.addItem>[0])
        return undefined
      case 'activityBar.removeItem':
        this.api.activityBar.removeItem(String(args.id))
        return undefined
    }
  }

  // ─── Subscription registry helpers ────────────────────────────────────

  private trackSub(subId: string, method: string, hostDispose: () => void): void {
    // Idempotent: a second registration under the same id silently
    // replaces — mirrors `PluginRegistry.trackSubscription` semantics.
    const existing = this.subscriptions.get(subId)
    if (existing) {
      try { existing.hostDispose() } catch { /* swallow */ }
    }
    this.subscriptions.set(subId, { hostDispose, method })
  }

  private removeSub(subId: string): void {
    const sub = this.subscriptions.get(subId)
    if (!sub) return
    try { sub.hostDispose() } catch (err) {
      this.warn('removeSub disposer threw', err)
    }
    this.subscriptions.delete(subId)
  }
}
