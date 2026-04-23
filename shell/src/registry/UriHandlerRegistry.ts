// src/registry/UriHandlerRegistry.ts
// Maps custom URI schemes (e.g. `nexus`) to plugin-provided handlers so
// deep links like `nexus://note/some-path` route to the right plugin.
//
// WI-13 / Phase 2 §5.3 — port of the legacy `dispatch_uri` +
// `list_plugin_uri_handlers` pattern. The legacy contribution registry
// (app/src/contributions/registry.ts) used first-match-wins semantics
// keyed by handler id; we follow the same rule, keyed by `(scheme,
// pluginId)` so a plugin can re-register its own handler idempotently
// during hot-reload but two distinct plugins cannot silently shadow
// each other's deep-link handlers.
//
// WI-19 — `dispatch` now consults `activationTriggers` for `onUri:<scheme>`
// before looking up the handler. This is async-friendly: when a deferred
// plugin owns the scheme, the trigger fires, the plugin's `activate()`
// runs (which calls `register(scheme, ...)`), and the freshly-registered
// handler is invoked. When nothing is gated, the path is unchanged.
//
// Wiring:
//   - `api.uri.register(scheme, handler)` (see PluginAPI.ts) calls
//     `register(scheme, pluginId, handler)` and the returned unsub is
//     tracked via `PluginRegistry.trackSubscription` so plugin
//     deactivation sweeps the entry automatically (matches the
//     `api.kernel.on` lifecycle).
//   - `unregisterByPlugin(pluginId)` exists for callers that prefer the
//     ownership-sweep model (`PluginRegistry.unregisterAll`); both
//     paths are safe to use together because the per-handler unsub is
//     idempotent and a no-op once the entry is gone.
//   - The Tauri-side bridge (deep-link plugin or a future Rust command)
//     is responsible for delivering the URL string to the shell, which
//     then constructs a `URL` and calls `dispatch(url)`. See the WI-13
//     report for the deferred Tauri wiring.

import { activationTriggers } from '../host/ActivationTriggers'

export type UriHandler = (uri: URL) => void | Promise<void>

interface UriHandlerEntry {
  pluginId: string
  handler: UriHandler
}

export class UriHandlerRegistry {
  // Key is the canonical scheme (lowercase, no trailing colon). At most
  // one entry per scheme — first-match-wins, matching legacy SI-2.
  private handlers = new Map<string, UriHandlerEntry>()

  /**
   * Register a handler for `scheme`. Returns an idempotent unsub.
   *
   * Conflict policy (matches legacy `registerUriHandler` SI-2):
   *   - If the same `pluginId` re-registers for the same scheme, the
   *     existing entry is replaced (idempotent re-register; supports
   *     hot-reload of a single plugin).
   *   - If a *different* plugin tries to register for an already-claimed
   *     scheme, the registration is rejected with a console warning and
   *     a no-op disposable is returned. Two plugins cannot silently
   *     shadow each other's deep-link handlers.
   */
  register(scheme: string, pluginId: string, handler: UriHandler): () => void {
    const canonical = canonicalizeScheme(scheme)
    if (!canonical) {
      console.warn(`[UriHandlerRegistry] register: empty/invalid scheme '${scheme}'`)
      return () => {}
    }

    const existing = this.handlers.get(canonical)
    if (existing && existing.pluginId !== pluginId) {
      console.warn(
        `[UriHandlerRegistry] scheme '${canonical}' already owned by ` +
          `'${existing.pluginId}' — ignoring duplicate registration from '${pluginId}'`,
      )
      return () => {}
    }

    const entry: UriHandlerEntry = { pluginId, handler }
    this.handlers.set(canonical, entry)

    let disposed = false
    return () => {
      if (disposed) return
      disposed = true
      // Only delete if the entry is still ours — covers the case where
      // the plugin was reloaded and a fresh entry replaced this one
      // before the old unsub fired.
      const current = this.handlers.get(canonical)
      if (current === entry) {
        this.handlers.delete(canonical)
      }
    }
  }

  /**
   * Look up the handler for `uri.protocol` and invoke it. Returns true
   * if a handler was found (regardless of whether the handler itself
   * succeeds — handler errors are logged, not rethrown, so a bad plugin
   * can't break the deep-link dispatch loop for the rest of the app).
   *
   * Returns false if no handler is registered for the URL's scheme.
   */
  dispatch(uri: URL): boolean {
    // `URL.protocol` includes the trailing colon (`nexus:`). Strip it
    // and lowercase to match the canonical key.
    const scheme = canonicalizeScheme(uri.protocol)
    if (!scheme) return false

    // WI-19 — wake any plugin gated on `onUri:<scheme>` first. This
    // path is fire-and-forget so `dispatch` keeps its sync `boolean`
    // return shape: when a deferred plugin owns the scheme, we kick off
    // activation, then dispatch the URL once the plugin has registered
    // its handler. We return `true` *optimistically* in that case
    // because a handler is on the way; callers that need a strict
    // "handler ran" signal can subscribe to the eventBus instead.
    const triggerKey = `onUri:${scheme}`
    if (activationTriggers.hasPending(triggerKey)) {
      activationTriggers
        .fire(triggerKey)
        .then(() => this.invoke(scheme, uri))
        .catch((err) => {
          console.error(
            `[UriHandlerRegistry] activation trigger for '${scheme}' threw:`,
            err,
          )
        })
      return true
    }

    return this.invoke(scheme, uri)
  }

  /** Inner dispatch — assumes activation is already settled. */
  private invoke(scheme: string, uri: URL): boolean {
    const entry = this.handlers.get(scheme)
    if (!entry) return false

    try {
      const result = entry.handler(uri)
      // Async handler — surface rejections without rethrowing so
      // `dispatch` stays sync and best-effort.
      if (result && typeof (result as Promise<void>).then === 'function') {
        ;(result as Promise<void>).catch((err) => {
          console.error(
            `[UriHandlerRegistry] handler for '${scheme}' (plugin '${entry.pluginId}') threw:`,
            err,
          )
        })
      }
    } catch (err) {
      console.error(
        `[UriHandlerRegistry] handler for '${scheme}' (plugin '${entry.pluginId}') threw:`,
        err,
      )
    }
    return true
  }

  /**
   * Sweep every handler owned by `pluginId`. Used by
   * `PluginRegistry.unregisterAll` as a belt-and-braces cleanup path —
   * the per-handler unsub returned from `register` already handles the
   * common case via `trackSubscription`.
   */
  unregisterByPlugin(pluginId: string): void {
    for (const [scheme, entry] of this.handlers) {
      if (entry.pluginId === pluginId) {
        this.handlers.delete(scheme)
      }
    }
  }

  /** Diagnostic — list all registered (scheme, pluginId) pairs. */
  all(): Array<{ scheme: string; pluginId: string }> {
    return [...this.handlers.entries()].map(([scheme, e]) => ({
      scheme,
      pluginId: e.pluginId,
    }))
  }

  /** True iff a handler is registered for the canonical form of `scheme`. */
  has(scheme: string): boolean {
    const canonical = canonicalizeScheme(scheme)
    return canonical ? this.handlers.has(canonical) : false
  }
}

/**
 * Canonical scheme form: lowercase, trailing colon stripped. Returns
 * null for empty/invalid input. `URL.protocol` includes the trailing
 * `:` (`'nexus:'`); manifest values typically don't (`'nexus'`). We
 * accept either and normalise here so callers don't have to think about
 * it.
 */
export function canonicalizeScheme(scheme: string): string {
  if (!scheme) return ''
  const trimmed = scheme.trim().toLowerCase()
  return trimmed.endsWith(':') ? trimmed.slice(0, -1) : trimmed
}

// Process-wide singleton, mirroring the `slotRegistry` pattern. The
// shell wires it through `PluginAPI` for the per-plugin surface and is
// free to reach for the singleton from a Tauri-event listener for the
// dispatch path.
export const uriHandlerRegistry = new UriHandlerRegistry()
