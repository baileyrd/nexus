/**
 * Wire protocol for the community-plugin sandbox (WI-30).
 *
 * Shared between:
 *   - the host-side `SandboxOrchestrator` (shell/src/host/sandbox/, WI-30b)
 *   - the guest-side bootstrap runtime (`./runtime.ts`, WI-30c)
 *
 * The envelope and handshake are deliberately tiny and JSON-cloneable so
 * `postMessage`'s `structuredClone` can carry them across the null-origin
 * iframe boundary without gymnastics.
 *
 * Keep this file importable from both realms — no React, no DOM types,
 * no Node globals. Pure value/type-level declarations only.
 */

import type { IpcErrorEnvelope } from '../generated/IpcErrorEnvelope';

/**
 * Sandbox-protocol version. Bumped when the envelope shape or the
 * handshake semantics change in a non-additive way. The ABI version
 * plugins see (`PLUGIN_API_VERSION`) is versioned separately — see
 * docs/wi30-sandbox-design.md §5.4.
 */
export const SANDBOX_PROTOCOL_VERSION = 1 as const;

/** Direction tag on every envelope. Advisory — routing uses `event.source`. */
export type RpcDirection = 'g2h' | 'h2g';

/** Request envelope — plugin calls a host method, or host dispatches to a guest handler. */
export interface RpcRequestEnvelope {
  id: string;
  dir: RpcDirection;
  kind: 'req';
  /** Dotted method name. Matches `SandboxedPluginContext` shape: `"commands.register"`, `"platform.fs.readText"`, etc. */
  method: string;
  args: unknown;
}

/** Successful response to a prior `req`. Correlated by `id`. */
export interface RpcResponseOkEnvelope {
  id: string;
  dir: RpcDirection;
  kind: 'res';
  ok: true;
  result: unknown;
}

/** Failure response — reuses {@link IpcErrorEnvelope} so sandbox errors match kernel errors. */
export interface RpcResponseErrEnvelope {
  id: string;
  dir: RpcDirection;
  kind: 'res';
  ok: false;
  error: IpcErrorEnvelope;
}

/** Event envelope — host → guest only, not correlated to any request. */
export interface RpcEventEnvelope {
  id: string;
  dir: 'h2g';
  kind: 'evt';
  topic: string;
  subscriptionId: string;
  payload: unknown;
}

/** System messages carry lifecycle signals, not application calls. */
export type RpcSystemKind =
  | 'handshake/hello'
  | 'handshake/accept'
  | 'handshake/reject'
  | 'ping'
  | 'pong'
  | 'suspend'
  | 'resume'
  | 'unload';

export interface RpcSystemEnvelope {
  id: string;
  dir: RpcDirection;
  kind: 'sys';
  system: RpcSystemKind;
  data?: unknown;
}

/** Every frame on the wire is one of these. Discriminant is `kind`. */
export type RpcEnvelope =
  | RpcRequestEnvelope
  | RpcResponseOkEnvelope
  | RpcResponseErrEnvelope
  | RpcEventEnvelope
  | RpcSystemEnvelope;

// ─── Handshake payloads ─────────────────────────────────────────────────────

/** Body of the guest's opening `handshake/hello`. */
export interface HandshakeHelloData {
  protocolVersion: number;
}

/**
 * Body of the host's `handshake/accept`. Carries the plugin-instance id
 * the guest will quote on every subsequent envelope, plus the method
 * catalog the guest uses to generate the client-side proxy.
 */
export interface HandshakeAcceptData {
  pluginInstanceId: string;
  /** Plugin id from the manifest (stable across runs). */
  pluginId: string;
  /** Plugin-author ABI version — what the plugin code was compiled against. */
  apiVersion: number;
  /** Negotiated wire protocol version (host ∩ guest). */
  protocolVersion: number;
  /** Dotted method names the host exposes. Guest builds a proxy from this list. */
  methods: ReadonlyArray<string>;
}

/** Body of a `handshake/reject` — host refused to activate the plugin. */
export interface HandshakeRejectData {
  error: IpcErrorEnvelope;
}

// ─── Type guards ────────────────────────────────────────────────────────────
//
// These are kept here (not in runtime.ts) so WI-30b can reuse them
// host-side without pulling the plugin bootstrap module.

export function isRpcEnvelope(value: unknown): value is RpcEnvelope {
  if (typeof value !== 'object' || value === null) return false;
  const v = value as { id?: unknown; kind?: unknown };
  if (typeof v.id !== 'string') return false;
  return (
    v.kind === 'req' || v.kind === 'res' || v.kind === 'evt' || v.kind === 'sys'
  );
}

export function isRpcSystem(env: RpcEnvelope): env is RpcSystemEnvelope {
  return env.kind === 'sys';
}
