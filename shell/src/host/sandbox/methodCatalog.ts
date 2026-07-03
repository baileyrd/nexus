// shell/src/host/sandbox/methodCatalog.ts
//
// WI-30b — enumerates every `api.*` surface that crosses the sandbox
// RPC boundary, per docs/wi30-sandbox-design.md §5.2.
//
// This file is TYPES + CONST LISTS ONLY. The router holds the runtime
// dispatch table; the capability guard holds the method→capability map.
// Keeping those three concerns separate means the author-facing ABI
// (@nexus/extension-api) can evolve without forcing either of the
// enforcement files to move in lockstep.
//
// Subscription pattern:
//   Handlers are non-serializable (closures). Every method that takes
//   a handler instead takes a `handlerSub` / `renderSub` / `handleSub`
//   — a guest-generated subscriptionId string. The guest stores
//   `subscriptionId → fn` locally; the host stores the real subscription
//   disposer under the same id. When the real subscription fires, the
//   host posts an `event` envelope carrying the subscriptionId in its
//   `id` field; the guest dispatches to its local fn. This pattern is
//   copied from `PluginAPI.ts:253-287` which already proves the
//   idempotent-unsub dance works for `api.kernel.on`.
//
// NOT in this catalog (flagged by the router as `unknown_method`):
//   - `views.register` with a `component: ComponentType` field —
//     a React function reference can't cross structured-clone. Sandbox
//     plugins use `views.registerPanel` with a PanelNode render
//     subscription instead (§6 decision: PanelNode-only).
//   - Direct `workspace.*` / `viewRegistry.*` method access — these are
//     live object references in the shell realm. A future snapshot
//     adapter (`workspace.getInfo()`, `workspace.onChanged(sub)`) will
//     land when a sandboxed plugin needs them; until then the router
//     returns `{ kind: 'unknown_method' }` with a hint in `message`.
//   - `configuration.*` — deferred until WI-30c wires the
//     configuration service into the author-facing sandbox proxy.
//     Today's first-party plugins call it synchronously; crossing the
//     boundary needs a subscription for `onChange` which is non-trivial
//     under the §5.6 pattern.

import type {
  PlatformDirEntry,
  PlatformOpenFileOptions,
  PlatformOpenDirectoryOptions,
  PlatformSaveFileOptions,
} from '@nexus/extension-api'

// ─── Shared payload shapes ───────────────────────────────────────────────────

export interface NotificationShape {
  message: string
  type?: 'info' | 'warning' | 'error' | 'success'
  duration?: number
  actions?: Array<{ label: string; command: string }>
}

export interface StatusBarItemConfig {
  id: string
  slot: 'left' | 'right'
  priority: number
  text?: string
  tooltip?: string
  command?: string
  className?: string
}

export interface ActivityBarItemConfig {
  id: string
  icon?: string
  iconPath?: string
  iconName?: string
  title: string
  viewId: string
  priority: number
  placement?: 'top' | 'bottom'
  command?: string
}

// ─── Method catalog type ─────────────────────────────────────────────────────
//
// The keys enumerate every plugin-to-host call the sandbox supports.
// `args` is what the guest posts in `payload`; `returns` is what the
// host posts back in `payload` on a successful response.

export interface SandboxMethodCatalog {
  // ── Commands ────────────────────────────────────────────────────────────
  'commands.register': {
    args: { id: string; handlerSub: string }
    returns: void
  }
  'commands.execute': {
    args: { id: string; args: unknown[] }
    returns: unknown
  }
  'commands.all': {
    args: Record<string, never>
    returns: Array<{ id: string; title: string; category?: string; keybinding?: string; pluginId: string }>
  }

  // ── Kernel bridge ───────────────────────────────────────────────────────
  'kernel.invoke': {
    args: { pluginId: string; commandId: string; args: unknown; timeoutMs?: number }
    returns: unknown
  }
  'kernel.on': {
    args: { topicPrefix: string; handlerSub: string }
    returns: { subscriptionId: string }
  }
  'kernel.off': {
    args: { subscriptionId: string }
    returns: void
  }
  'kernel.available': {
    args: Record<string, never>
    returns: boolean
  }

  // ── Platform: filesystem ────────────────────────────────────────────────
  'platform.fs.readText': {
    args: { path: string }
    returns: string
  }
  'platform.fs.writeText': {
    args: { path: string; content: string }
    returns: void
  }
  'platform.fs.readDir': {
    args: { path: string }
    returns: PlatformDirEntry[]
  }
  'platform.fs.exists': {
    args: { path: string }
    returns: boolean
  }
  'platform.fs.mkdir': {
    args: { path: string; recursive?: boolean }
    returns: void
  }
  'platform.fs.remove': {
    args: { path: string }
    returns: void
  }
  'platform.fs.rename': {
    args: { from: string; to: string }
    returns: void
  }

  // ── Platform: dialog ────────────────────────────────────────────────────
  'platform.dialog.openFile': {
    args: { options?: PlatformOpenFileOptions }
    returns: string | string[] | null
  }
  'platform.dialog.openDirectory': {
    args: { options?: PlatformOpenDirectoryOptions }
    returns: string | string[] | null
  }
  'platform.dialog.saveFile': {
    args: { options?: PlatformSaveFileOptions }
    returns: string | null
  }

  // ── Platform: window ────────────────────────────────────────────────────
  'platform.window.minimize': {
    args: Record<string, never>
    returns: void
  }
  'platform.window.toggleMaximize': {
    args: Record<string, never>
    returns: void
  }
  'platform.window.close': {
    args: Record<string, never>
    returns: void
  }
  'platform.window.isMaximized': {
    args: Record<string, never>
    returns: boolean
  }

  // ── Platform: shell ─────────────────────────────────────────────────────
  'platform.shell.openExternal': {
    args: { target: string }
    returns: void
  }

  // ── Platform: net (C81) ─────────────────────────────────────────────────
  'platform.net.request': {
    args: { method: string; url: string; headers?: Record<string, string>; body?: string }
    returns: { status: number; headers: Record<string, string>; body: string }
  }

  // ── Events ──────────────────────────────────────────────────────────────
  'events.on': {
    args: { event: string; handlerSub: string }
    returns: { subscriptionId: string }
  }
  'events.off': {
    args: { subscriptionId: string }
    returns: void
  }
  'events.emit': {
    args: { event: string; payload: unknown }
    returns: void
  }

  // ── Storage (per-plugin, sandboxed — no cap required) ──────────────────
  'storage.get': {
    args: { key: string }
    returns: string | null
  }
  'storage.set': {
    args: { key: string; value: string }
    returns: void
  }
  'storage.delete': {
    args: { key: string }
    returns: void
  }
  'storage.clear': {
    args: Record<string, never>
    returns: void
  }

  // ── Notifications ──────────────────────────────────────────────────────
  'notifications.show': {
    args: { notification: NotificationShape }
    returns: void
  }

  // ── Context keys ───────────────────────────────────────────────────────
  'context.set': {
    args: { key: string; value: unknown }
    returns: void
  }
  'context.get': {
    args: { key: string }
    returns: unknown
  }
  'context.evaluate': {
    args: { expression: string }
    returns: boolean
  }

  // ── Status bar ─────────────────────────────────────────────────────────
  'statusBar.createItem': {
    args: { config: StatusBarItemConfig }
    returns: { handleSub: string }
  }

  // ── URI handlers ───────────────────────────────────────────────────────
  'uri.register': {
    args: { scheme: string; handlerSub: string }
    returns: { subscriptionId: string }
  }

  // ── Declarative views (PanelNode only) ─────────────────────────────────
  'views.registerPanel': {
    args: { viewId: string; slot: string; renderSub: string; priority?: number }
    returns: void
  }

  // ── Input ──────────────────────────────────────────────────────────────
  'input.prompt': {
    args: { message: string; placeholder?: string }
    returns: string | null
  }
  'input.confirm': {
    args: { message: string }
    returns: boolean
  }

  // ── Activity bar ───────────────────────────────────────────────────────
  'activityBar.addItem': {
    args: { config: ActivityBarItemConfig }
    returns: void
  }
  'activityBar.removeItem': {
    args: { id: string }
    returns: void
  }
}

export type SandboxMethodName = keyof SandboxMethodCatalog

/**
 * Runtime list of every supported method name. The handshake payload
 * carries a copy so the guest can generate its proxy without a second
 * round trip.
 *
 * Kept in lockstep with `SandboxMethodCatalog` by the test suite — any
 * key that appears in one but not the other fails the
 * "catalog vs METHOD_NAMES parity" assertion.
 */
export const SANDBOX_METHOD_NAMES = [
  'commands.register',
  'commands.execute',
  'commands.all',
  'kernel.invoke',
  'kernel.on',
  'kernel.off',
  'kernel.available',
  'platform.fs.readText',
  'platform.fs.writeText',
  'platform.fs.readDir',
  'platform.fs.exists',
  'platform.fs.mkdir',
  'platform.fs.remove',
  'platform.fs.rename',
  'platform.dialog.openFile',
  'platform.dialog.openDirectory',
  'platform.dialog.saveFile',
  'platform.window.minimize',
  'platform.window.toggleMaximize',
  'platform.window.close',
  'platform.window.isMaximized',
  'platform.shell.openExternal',
  'platform.net.request',
  'events.on',
  'events.off',
  'events.emit',
  'storage.get',
  'storage.set',
  'storage.delete',
  'storage.clear',
  'notifications.show',
  'context.set',
  'context.get',
  'context.evaluate',
  'statusBar.createItem',
  'uri.register',
  'views.registerPanel',
  'input.prompt',
  'input.confirm',
  'activityBar.addItem',
  'activityBar.removeItem',
] as const satisfies ReadonlyArray<SandboxMethodName>

/**
 * Method names the sandbox explicitly rejects with a hint, so the
 * router can return `unknown_method` with a useful `message` rather
 * than a bare "not found". Populated from §5.2's not-crossing list.
 */
export const SANDBOX_REJECTED_METHODS: Readonly<Record<string, string>> = {
  'views.register':
    'Function-component views cannot cross the sandbox. Use `api.views.registerPanel` with a PanelNode render subscription instead (design doc §6).',
  'workspace.getRoot':
    'Direct workspace access is not available in the sandbox. A read-only snapshot adapter is planned — see design doc §5.2.',
  'viewRegistry.register':
    'Direct viewRegistry access is not available in the sandbox. Use `api.views.registerPanel` instead.',
}
