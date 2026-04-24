// shell/src/host/sandbox/protocol.ts
//
// WI-30b — wire-level types for the community-plugin sandbox RPC.
//
// Per docs/wi30-sandbox-design.md §5.1. The sandbox iframe is a
// null-origin realm; every call between host and plugin crosses
// `postMessage` and therefore crosses structured-clone. Functions do
// not clone — plugin handlers cross the boundary as subscription ids
// (§5.6). This file is protocol-only: no imports from DOM, React, or
// Tauri. That keeps it safe to consume from both the host side AND
// (eventually) the guest bridge shipped inside the sandbox's srcdoc.
//
// The envelope shape is intentionally richer than the design doc's
// historical sketch: the design doc pairs `{ok, result|error}` for
// responses; this implementation flattens `kind: 'response'` and lets
// `error` be absent for success. The two forms are equivalent and the
// flat form round-trips through a single `RpcEnvelope` type which is
// simpler for the router to switch on.
//
// Not in scope here:
//   - The iframe itself (WI-30d).
//   - Capability → method map (lives in `capabilityGuard.ts`).
//   - Method argument/return types (lives in `methodCatalog.ts`).
//   - Author-facing plugin API types (owned by `packages/nexus-extension-api`).

import type { IpcErrorEnvelope } from '@nexus/extension-api'

/**
 * Protocol version for the sandbox RPC wire. Bumped whenever the
 * envelope shape or method dispatch semantics change in a way guest
 * bootstraps from older hosts would misread. `PLUGIN_API_VERSION` is
 * orthogonal — see design doc §5.4.
 */
export const SANDBOX_PROTOCOL_VERSION = 1 as const

export type SandboxProtocolVersion = typeof SANDBOX_PROTOCOL_VERSION

export type RpcDirection = 'host-to-plugin' | 'plugin-to-host'

export type RpcKind =
  | 'handshake'   // initial version negotiation (direction + handshakeKind carry the sub-step)
  | 'request'     // RPC method call expecting a response
  | 'response'    // reply to a request (ok when `error` is undefined, err otherwise)
  | 'event'       // fire-and-forget notification; used for kernel/event subscription delivery
  | 'dispose'     // teardown a subscription (ack via matching response unless id omitted)

/**
 * RPC-specific error kinds. `IpcErrorEnvelope.kind` is the authoritative
 * vocabulary for errors that originate from the kernel (timeout,
 * plugin_crashed, capability_denied, dispatch_failed, serialization,
 * unknown); the sandbox reuses those verbatim when it proxies a kernel
 * failure. The extra kinds below cover failures that can only happen at
 * the RPC layer itself, before the call reaches any Tauri bridge.
 */
export type RpcErrorKind =
  | 'unknown_method'         // method name not in the catalog
  | 'capability_denied'      // host-side cap check failed (also aliased by kernel IpcErrorKind)
  | 'protocol_mismatch'      // handshake sandbox-protocol version not supported
  | 'api_version_mismatch'   // handshake PLUGIN_API_VERSION not supported
  | 'timeout'                // RPC exceeded its response window (also aliased by kernel IpcErrorKind)
  | 'serialization_failed'   // argument or result could not survive structured-clone
  | 'plugin_disposed'        // router was torn down while the request was in flight
  | 'dispatch_failed'        // generic routing failure (matches IpcErrorKind)

export interface RpcErrorEnvelope {
  kind: RpcErrorKind
  message: string
  /** Mirrors `IpcErrorEnvelope.retryable`; only `timeout` is retryable today. */
  retryable: boolean
  /** Optional, for parity with `IpcErrorEnvelope.plugin_id` / `.command`. */
  pluginId?: string
  method?: string
}

/**
 * The union of error shapes the sandbox may return on a `response`.
 * When a request is proxied through `kernel.invoke`, the kernel's
 * authoritative `IpcErrorEnvelope` is returned verbatim; RPC-layer
 * failures use the local `RpcErrorEnvelope`. Callers must branch on
 * shape: `IpcErrorEnvelope` carries `plugin_id` + `command` as
 * snake_case fields (wire-stable from ts-rs), whereas `RpcErrorEnvelope`
 * uses camelCase.
 */
export type RpcResponseError = RpcErrorEnvelope | IpcErrorEnvelope

/**
 * Message envelope shared by every frame in both directions.
 *
 * Fields:
 *   - `id` — correlation id for request/response; also used on events
 *     to carry the `subscriptionId` when delivering a kernel/event
 *     notification. May be omitted on unilateral `dispose` frames.
 *   - `direction` — advisory. Routing uses `event.source` identity
 *     on the host side; direction lets crash dumps self-describe.
 *   - `kind` — see RpcKind.
 *   - `method` — dotted name from the method catalog. Required on
 *     request and event; optional on handshake (absent), response
 *     (echoes the request's method for diagnostic logging),
 *     dispose (the method that owned the subscription).
 *   - `payload` — method-specific body. Round-trips via
 *     structured-clone, so Date / Map / Set / ArrayBuffer survive.
 *   - `error` — present on failed responses and on dispose frames
 *     that want to signal a teardown reason.
 */
export interface RpcEnvelope {
  id: string
  direction: RpcDirection
  kind: RpcKind
  method?: string
  payload?: unknown
  error?: RpcResponseError
}

// ─── Handshake sub-payloads ──────────────────────────────────────────────────
//
// Handshake is intentionally its own `RpcKind` rather than a method.
// The guest does not have a method catalog until the host answers the
// hello — method names would be unresolvable at that point.

export interface HandshakeHello {
  /** Protocol version the guest stub was built against. */
  protocolVersion: number
  /** Bundle-author ABI marker, from `@nexus/extension-api.PLUGIN_API_VERSION`. */
  apiVersion: number
  /** Guest-assigned nonce so the accept can be correlated. */
  nonce: string
}

export interface HandshakeAccept {
  protocolVersion: SandboxProtocolVersion
  /** Host-assigned identity for the plugin instance; echoed on every frame. */
  pluginInstanceId: string
  /** Method names the host will honor; guest proxy is generated from this. */
  methods: ReadonlyArray<string>
  /** Nonce from the corresponding hello. */
  nonce: string
}

export interface HandshakeReject {
  nonce: string
  reason: 'protocol_mismatch' | 'api_version_mismatch' | 'dispatch_failed'
  message: string
}

export type HandshakePayload = HandshakeHello | HandshakeAccept | HandshakeReject

// ─── Construction helpers ────────────────────────────────────────────────────
//
// Kept dep-free on purpose — the guest bridge (WI-30d) will ship the
// same file via srcdoc and must run without pulling in @nexus/*.

export function makeRequest(
  id: string,
  method: string,
  payload: unknown,
  direction: RpcDirection = 'plugin-to-host',
): RpcEnvelope {
  return { id, direction, kind: 'request', method, payload }
}

export function makeResponse(
  id: string,
  payload: unknown,
  direction: RpcDirection = 'host-to-plugin',
  method?: string,
): RpcEnvelope {
  return { id, direction, kind: 'response', method, payload }
}

export function makeErrorResponse(
  id: string,
  error: RpcResponseError,
  direction: RpcDirection = 'host-to-plugin',
  method?: string,
): RpcEnvelope {
  return { id, direction, kind: 'response', method, error }
}

export function makeEvent(
  subscriptionId: string,
  method: string,
  payload: unknown,
  direction: RpcDirection = 'host-to-plugin',
): RpcEnvelope {
  return { id: subscriptionId, direction, kind: 'event', method, payload }
}

export function makeHandshakeHello(hello: HandshakeHello): RpcEnvelope {
  return {
    id: hello.nonce,
    direction: 'plugin-to-host',
    kind: 'handshake',
    payload: hello,
  }
}

export function makeHandshakeAccept(accept: HandshakeAccept): RpcEnvelope {
  return {
    id: accept.nonce,
    direction: 'host-to-plugin',
    kind: 'handshake',
    payload: accept,
  }
}

export function makeHandshakeReject(reject: HandshakeReject): RpcEnvelope {
  return {
    id: reject.nonce,
    direction: 'host-to-plugin',
    kind: 'handshake',
    payload: reject,
    error: {
      kind: reject.reason,
      message: reject.message,
      retryable: false,
    },
  }
}

/**
 * Type-narrow a value received over `postMessage`. Protects the router
 * from malformed frames (non-object, missing fields, unknown `kind`).
 */
export function isRpcEnvelope(value: unknown): value is RpcEnvelope {
  if (!value || typeof value !== 'object') return false
  const v = value as Record<string, unknown>
  if (typeof v.id !== 'string') return false
  if (v.direction !== 'host-to-plugin' && v.direction !== 'plugin-to-host') return false
  switch (v.kind) {
    case 'handshake':
    case 'request':
    case 'response':
    case 'event':
    case 'dispose':
      return true
    default:
      return false
  }
}

/**
 * Duck-type discriminator between RpcErrorEnvelope and IpcErrorEnvelope.
 * The kernel envelope ships `plugin_id` + `command` as snake_case keys;
 * the RPC envelope uses camelCase `pluginId` / `method`.
 */
export function isIpcErrorEnvelope(error: RpcResponseError): error is IpcErrorEnvelope {
  return 'plugin_id' in error && 'command' in error
}
