// shell/src/host/sandbox/capabilityGuard.ts
//
// WI-30b — host-side fast-fail capability check for the sandbox RPC.
// Per docs/wi30-sandbox-design.md §5.3.
//
// This file declares the mapping from method name → required capability.
// The router consults it BEFORE routing to any `api.*` surface, so a
// capability-denied call never reaches the kernel (even though the
// kernel itself re-checks capabilities as the authoritative backstop
// — `crates/nexus-plugins/src/loader.rs`).
//
// `null` means "no capability required". These fall into three groups:
//   1. Pure RPC plumbing that cannot reach a privileged resource
//      (e.g. `commands.register`, `events.on`).
//   2. Per-plugin namespaced storage — writes land in
//      `localStorage['plugin:<id>:<key>']`; no other plugin can read it
//      and nothing leaves the origin.
//   3. UI-local side effects (`context.*`, `notifications.show`,
//      `input.*`, `statusBar.*`, `activityBar.*`, `views.registerPanel`)
//      — these manipulate in-process registries only.
//
// Capabilities come from the `Capability` enum in
// `@nexus/extension-api` (ts-rs-generated from `crates/nexus-plugin-api`).
// Strings used here MUST match the enum variants verbatim; a test in
// `sandboxProtocol.test.ts` asserts that.

import type { Capability } from '@nexus/extension-api'
import type { SandboxMethodName } from './methodCatalog'

// Note: sandbox protocol types (`RpcEnvelope`, `SANDBOX_PROTOCOL_VERSION`, …)
// are re-exported alongside `Capability` from `@nexus/extension-api`.
// This file does not need them directly but the shared import path
// guarantees host and guest see identical declarations.

/**
 * Placeholder for methods that don't map cleanly to an existing capability
 * but still warrant some form of gate before a richer capability vocabulary
 * lands (WI-31 follow-up). Today these are `null` (no gate); the type is
 * expressed as `Capability | null` so future edits flip `null` → enum
 * variant without widening anything.
 */
type RequiredCap = Capability | null

/**
 * Method → capability map. A method whose value is `null` skips the
 * guard entirely. Methods not present in this map are rejected with
 * `unknown_method` before the guard runs; that distinction keeps the
 * "no cap required" case from masking typos.
 */
export const METHOD_CAPABILITY_MAP: Record<SandboxMethodName, RequiredCap> = {
  // Commands — routing plumbing, no external reach. The actual command
  // handler will itself sit behind capability checks as implemented by
  // the plugin author; `commands.execute` dispatches through the host
  // registry which may route into kernel calls (which re-check).
  'commands.register': null,
  'commands.execute': null,
  'commands.all': null,

  // Kernel bridge — `kernel.invoke` requires `IpcCall` so consent is
  // surfaced up front; the kernel re-checks server-side. `kernel.on`
  // is subscribing to kernel events — no equivalent capability exists
  // yet (design doc §5.2 lists a future `EventSubscribe`), so left
  // null pending WI-31 extension. `kernel.off` is teardown-only.
  'kernel.invoke': 'IpcCall',
  'kernel.on': null,
  'kernel.off': null,
  'kernel.available': null,

  // Platform: filesystem — direct FsRead / FsWrite gates.
  'platform.fs.readText': 'FsRead',
  'platform.fs.writeText': 'FsWrite',
  'platform.fs.readDir': 'FsRead',
  'platform.fs.exists': 'FsRead',
  'platform.fs.mkdir': 'FsWrite',
  'platform.fs.remove': 'FsWrite',
  'platform.fs.rename': 'FsWrite',

  // Platform: dialog — no matching Capability variant yet; a user-
  // initiated OS picker is not a raw FS grant. Left null for now; a
  // future `DialogShow` capability would flip these.
  'platform.dialog.openFile': null,
  'platform.dialog.openDirectory': null,
  'platform.dialog.saveFile': null,

  // Platform: window — no matching Capability yet. Window minimize/
  // close is disruptive but not data-sensitive; a future
  // `WindowControl` capability would flip these.
  'platform.window.minimize': null,
  'platform.window.toggleMaximize': null,
  'platform.window.close': null,
  'platform.window.isMaximized': null,

  // Platform: shell — opening external URLs is a privacy-adjacent
  // action (DNS lookups, default handler). No matching capability
  // yet; a future `ShellOpen` would flip this.
  'platform.shell.openExternal': null,

  // Platform: net (C81) — brokered outbound HTTP, gated on NetHttp same
  // as the WASM host::http_request and the com.nexus.security::http_request
  // IPC handler it's implemented on top of. The kernel-side sandbox.toml
  // `[http]` policy (host allowlist, https-only, response-size cap) is the
  // authoritative backstop this guard can't see.
  'platform.net.request': 'NetHttp',

  // Events — intra-process pub/sub, no external reach.
  'events.on': null,
  'events.off': null,
  'events.emit': null,

  // Storage — per-plugin namespaced; cannot cross plugin boundaries.
  'storage.get': null,
  'storage.set': null,
  'storage.delete': null,
  'storage.clear': null,
  'storage.list': null,

  // Notifications — UI-local toast queue.
  'notifications.show': 'UiNotify',

  // Context keys — in-process map read/write.
  'context.set': null,
  'context.get': null,
  'context.evaluate': null,

  // Status bar / activity bar — UI registries, no external reach.
  'statusBar.createItem': null,

  // URI handlers — claiming a scheme doesn't leak data by itself; the
  // handler will receive URLs the user explicitly navigates to.
  'uri.register': null,

  // Declarative views — render output is sanitized by the host's
  // PanelNode dispatcher; no HTML escape hatch (design doc §6).
  'views.registerPanel': null,

  // Input — modal prompts; trusted UI path.
  'input.prompt': null,
  'input.confirm': null,

  // Activity bar — UI registry.
  'activityBar.addItem': null,
  'activityBar.removeItem': null,
}

/**
 * Lookup the capability required for a method. Returns `null` when no
 * capability is required, `undefined` when the method is not in the
 * catalog (caller should emit `unknown_method`).
 */
export function requiredCapabilityFor(method: string): RequiredCap | undefined {
  if (!(method in METHOD_CAPABILITY_MAP)) return undefined
  return METHOD_CAPABILITY_MAP[method as SandboxMethodName]
}

/**
 * Host-side precheck. True when the plugin may proceed with `method`.
 * The granted set is whatever the consent flow / `granted_caps.json`
 * populated at spawn time.
 */
export function checkCapability(
  method: string,
  grantedCaps: ReadonlySet<string>,
): { allowed: true } | { allowed: false; required: string } {
  const required = requiredCapabilityFor(method)
  if (required === undefined) {
    // Caller will emit `unknown_method` separately; guard shouldn't
    // double-handle. Treat as not-allowed so the router short-circuits.
    return { allowed: false, required: '__unknown_method__' }
  }
  if (required === null) return { allowed: true }
  if (grantedCaps.has(required)) return { allowed: true }
  return { allowed: false, required }
}
