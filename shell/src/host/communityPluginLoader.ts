// src/host/communityPluginLoader.ts
// Discovers and loads community plugins from ~/.nexus-shell/plugins/.
//
// Boot flow:
//   1. scanCommunityPlugins() — calls Rust to enumerate plugin directories
//   2. loadEnabledCommunityPlugins() — for each enabled one, reads the JS
//      bundle via plugin-fs and executes it via a Blob URL (avoids CSP issues
//      with file:// imports).  The module must export a default Plugin object.

import { invoke }        from '@tauri-apps/api/core'
import { readTextFile }  from '@tauri-apps/plugin-fs'
import { PLUGIN_API_VERSION } from '@nexus/extension-api'
import type { Plugin }   from '../types/plugin'
import type { SandboxOrchestrator } from './sandbox/SandboxOrchestrator'
import { parseManifestCapabilities } from '../plugins/nexus/pluginsMgmt/capabilityInfo'
import { assertValidPluginId } from './PluginAPI'
import { clientLogger } from './clientLogger'
import { getHost } from './shellHost'
import { getRegistry } from './shellRegistry'
import { runInstallTimeConsent } from '../plugins/core/capabilityPrompt'
import { PLUGIN_LIST_CHANGED_EVENT } from './pluginActivation'
import { eventBus } from './EventBus'

// ── Types ─────────────────────────────────────────────────────────────────────

export interface CommunityPluginManifest {
  id:          string
  name:        string
  version:     string
  main:        string
  enabled:     boolean
  description?: string
  author?:      string
  /**
   * Plugin API version the plugin targets. When absent ("legacy plugin"),
   * the shell logs a warn and continues; when present and != the shell's
   * `PLUGIN_API_VERSION`, the plugin is rejected with a
   * `PluginApiVersionError` before activation (WI-33).
   */
  apiVersion?: number
  /**
   * Declared capabilities (WI-31). Raw PascalCase strings matching the
   * ts-rs `Capability` union (`"FsRead"`, `"NetHttp"`, …). The Rust
   * scanner forwards this field verbatim from plugin.json; the TS side
   * filters unknown variants via `parseManifestCapabilities`.
   *
   *   - `undefined` — manifest did not declare a capabilities list
   *     (legacy plugin; treated as "(unknown)" in the UI, no consent
   *     prompt fires because we don't know what to ask about).
   *   - `[]`        — declared empty (runs with zero capabilities).
   *   - non-empty   — drives the install-time consent prompt.
   */
  capabilities?: string[]
  /**
   * Opt-in to iframe sandbox isolation (WI-30d). When `true` the
   * ExtensionHost routes this plugin through `SandboxOrchestrator`
   * instead of the dynamic-import path — the bundle runs in a
   * null-origin iframe and communicates with the host via the
   * postMessage RPC protocol defined in
   * `@nexus/extension-api/sandbox/protocol`.
   *
   * Default `false` for back-compat. Hello-world's migration to
   * `sandboxed: true` is tracked as WI-30e.
   *
   * First-party plugins (from `shell/src/plugins/{core,nexus}/`) are
   * never sandboxed — they run in the shell realm with full access.
   */
  sandboxed?: boolean
  /** Absolute path to the plugin's directory — injected by the Rust scanner */
  dir:          string
  /** Absolute path to plugin.json — injected by the Rust scanner */
  manifestPath: string
  /**
   * OI-15 — Ed25519 signature check result injected by the Rust scanner.
   * `undefined` means the Rust side is older (pre-OI-15) and did not
   * supply the field; treat as `'unsigned'`.
   *
   *   - `'unsigned'`          — no plugin.json.sig present; loads normally
   *   - `'verified'`          — signed by a key in TRUSTED_PUBLIC_KEYS
   *   - `'untrustedKey'`      — sig present but key not trusted; rejected at scan
   *   - `'invalidSignature'`  — sig file corrupt or crypto failure; rejected at scan
   */
  verificationStatus?: 'unsigned' | 'verified' | 'untrustedKey' | 'invalidSignature'
}

// ── API-version error (WI-33) ────────────────────────────────────────────────

/**
 * Thrown when a community plugin declares an `apiVersion` that differs
 * from the shell's `PLUGIN_API_VERSION`. The loader catches this and
 * records the plugin as unloadable so the PluginsMgmt + Settings views
 * can surface a clear "Incompatible" chip instead of a silent failure.
 *
 * Mirrors the kernel-side `PluginError::IncompatibleApiVersion` at
 * `crates/nexus-plugins/src/loader.rs:1539` — shell check is an
 * early-rejection mirror, not a replacement (kernel still gates WASM
 * plugins on the Rust side).
 */
export class PluginApiVersionError extends Error {
  readonly kind = 'api_version_mismatch' as const
  readonly pluginId: string
  readonly requested: number
  readonly supported: number

  constructor(pluginId: string, requested: number, supported: number) {
    super(
      `Plugin '${pluginId}' requires apiVersion ${requested}, ` +
      `but the shell supports ${supported}`,
    )
    this.name = 'PluginApiVersionError'
    this.pluginId = pluginId
    this.requested = requested
    this.supported = supported
    // Restore prototype chain under ES5-targeted transpilation.
    Object.setPrototypeOf(this, PluginApiVersionError.prototype)
  }
}

// Set of plugin ids we have already warned about for missing apiVersion,
// so a legacy plugin doesn't spam the console on every re-scan.
const warnedLegacyPlugins = new Set<string>()

/**
 * Compare a manifest's declared `apiVersion` against the shell's
 * `PLUGIN_API_VERSION`. Returns `{ ok: true }` when the plugin may be
 * loaded, otherwise an error the caller should propagate.
 *
 * Rules (mirroring the kernel at loader.rs:1534-1545):
 *   - Absent / undefined   → ok, with a one-shot console.warn (legacy).
 *   - Equal to shell const → ok.
 *   - Anything else        → error; caller should record the plugin as
 *                            unloadable with a typed
 *                            `PluginApiVersionError`.
 */
export function checkApiVersion(
  pluginId: string,
  apiVersion: number | undefined,
  supported: number = PLUGIN_API_VERSION,
): { ok: true } | { ok: false; error: PluginApiVersionError } {
  if (apiVersion === undefined || apiVersion === null) {
    if (!warnedLegacyPlugins.has(pluginId)) {
      warnedLegacyPlugins.add(pluginId)
      clientLogger.warn(
        `[CommunityLoader] '${pluginId}' declares no apiVersion — ` +
        `treating as legacy plugin. Add \`"apiVersion": ${supported}\` ` +
        `to plugin.json to opt in to the stable ABI.`,
      )
    }
    return { ok: true }
  }
  if (apiVersion === supported) return { ok: true }
  return {
    ok: false,
    error: new PluginApiVersionError(pluginId, apiVersion, supported),
  }
}

/**
 * Test-only: reset the one-shot "legacy plugin" warn memo so unit tests
 * can assert console output without cross-test bleed-through. Not part
 * of the public shell API; exported because the test file is a sibling
 * of the implementation.
 */
export function __resetLegacyWarnMemoForTests() {
  warnedLegacyPlugins.clear()
}

// Injected by vite.config.ts — absolute path to src/plugins/community/.
// Only valid in dev mode (replaced with the literal string at build time).
declare const __DEV_COMMUNITY_PLUGINS_DIR__: string

// ── Public API ────────────────────────────────────────────────────────────────

/**
 * Scan for community plugins from two sources:
 *   1. ~/.nexus-shell/plugins/          — user-installed plugins (all modes)
 *   2. src/plugins/community/ in repo   — dev-mode only, loaded from the project
 *      directly so you never need to copy files while iterating.
 *
 * Repo plugins with the same id as an installed plugin are deduplicated
 * (installed copy wins).
 */
export async function scanCommunityPlugins(): Promise<CommunityPluginManifest[]> {
  const results = await Promise.allSettled([
    invoke<CommunityPluginManifest[]>('scan_plugin_directory'),
    import.meta.env.DEV
      ? invoke<CommunityPluginManifest[]>('scan_plugin_directory_at', {
          dir: __DEV_COMMUNITY_PLUGINS_DIR__,
        })
      : Promise.resolve([] as CommunityPluginManifest[]),
  ])

  const installed: CommunityPluginManifest[] =
    results[0].status === 'fulfilled' ? results[0].value : []
  const fromRepo: CommunityPluginManifest[] =
    results[1].status === 'fulfilled' ? results[1].value : []

  if (results[0].status === 'rejected') {
    clientLogger.warn('[CommunityLoader] scan_plugin_directory failed:', (results[0] as PromiseRejectedResult).reason)
  }
  if (results[1].status === 'rejected') {
    clientLogger.warn('[CommunityLoader] scan_plugin_directory_at failed:', (results[1] as PromiseRejectedResult).reason)
  }

  // Merge: installed plugins take precedence; repo fills in anything not installed
  const installedIds = new Set(installed.map(m => m.id))
  const merged = [
    ...installed,
    ...fromRepo
      .filter(m => !installedIds.has(m.id))
      .map(m => ({ ...m, _source: 'repo' as const })),
  ]

  clientLogger.info(
    `[CommunityLoader] ${merged.length} plugin(s) discovered` +
    (import.meta.env.DEV ? ` (${fromRepo.length} from repo)` : '')
  )
  return merged
}

/**
 * Options for `loadEnabledCommunityPlugins`. `orchestrator` is optional
 * because the unsandboxed legacy path does not require it; callers that
 * intend to load sandboxed plugins MUST pass one or the sandboxed
 * manifests will be skipped with a warning.
 */
export interface LoadCommunityPluginsOptions {
  /**
   * Iframe orchestrator for sandboxed plugins (WI-30e). When a
   * manifest sets `sandboxed: true`, the loader calls
   * `orchestrator.load(...)` instead of dynamic-importing the bundle
   * into the shell realm. Omit when the caller is operating in a
   * non-UI context (tests, scans, …); sandboxed manifests are then
   * filtered out with a console warning.
   */
  orchestrator?: SandboxOrchestrator
  /**
   * Factory that produces a blob URL for the guest runtime bootstrap.
   * Allowing the caller to inject this keeps the loader decoupled
   * from how the shell ships the compiled
   * `bootstrapSandboxedPlugin` — in dev the caller can hand back a
   * live `new URL('.../sandbox/runtime.ts', import.meta.url)`; in
   * production it will be a precompiled ESM bundle shipped alongside
   * the shell. Required when `orchestrator` is set.
   */
  getRuntimeUrl?: () => Promise<string>
}

/**
 * Load all *enabled* community plugins.
 *
 * Routing (WI-30e):
 *   - `manifest.sandboxed === true` — forward to
 *     `orchestrator.load(...)`. The plugin runs in a null-origin
 *     iframe; its host-side effects (commands, panels, notifications)
 *     are installed via the router's RPC bridge. The loader does NOT
 *     return a `Plugin` for sandboxed entries because the shell's
 *     `ExtensionHost` lifecycle does not apply — the orchestrator
 *     owns load/unload.
 *   - `manifest.sandboxed` unset/false — legacy dynamic-import path.
 *     Reads the bundle via the fs plugin, wraps it in a Blob URL,
 *     imports it, and returns the default-exported `Plugin` to the
 *     caller for `ExtensionHost.loadAll`.
 *
 * The JS bundle in the legacy path must export a Plugin as default:
 *   export default { manifest, activate, deactivate }
 */
export async function loadEnabledCommunityPlugins(
  manifests: CommunityPluginManifest[],
  options: LoadCommunityPluginsOptions = {},
): Promise<Plugin[]> {
  const enabled = manifests.filter(m => m.enabled)
  if (enabled.length === 0) return []

  const results = await Promise.allSettled(
    enabled.map(m => loadOnePlugin(m, options))
  )

  const plugins: Plugin[] = []
  for (const [i, result] of results.entries()) {
    if (result.status === 'fulfilled') {
      // Sandboxed entries resolve to `null` — they've been loaded
      // through the orchestrator and don't participate in the
      // ExtensionHost's in-realm plugin list.
      if (result.value) plugins.push(result.value)
    } else {
      clientLogger.error(
        `[CommunityLoader] ✗ ${enabled[i].id}:`,
        result.reason
      )
    }
  }

  return plugins
}

// ── Rescan (C80) ─────────────────────────────────────────────────────────────

/** Outcome of {@link rescanCommunityPlugins}. */
export interface RescanResult {
  /** Freshly-scanned manifest list (every plugin found, enabled or not). */
  manifests: CommunityPluginManifest[]
  /** Ids of plugins that were newly discovered and successfully activated. */
  added: string[]
  /** Ids of newly-discovered plugins the user denied consent for. */
  denied: string[]
  /** `{ id, error }` for newly-discovered plugins that failed to load. */
  errors: Array<{ id: string; error: string }>
}

/**
 * C80 — re-scan `~/.nexus-shell/plugins/` for community plugins dropped
 * in since boot, and activate the ones that are new.
 *
 * Deliberately does **not** attempt to hot-swap an already-loaded
 * plugin's changed code — that would mean unwinding whatever its
 * `activate()` already did (registered commands, subscriptions, DOM
 * nodes, …) before re-running it, which the shell has no general
 * mechanism for. A plugin already in `host.listAll()` is left alone;
 * only ids the host has never seen this session are loaded. Editing an
 * existing script plugin still needs a shell reload — this closes the
 * *other* half of the C80 gap (install a fifth plugin without
 * restarting), matching the kernel-side `nexus plugin dev` verb, which
 * genuinely hot-swaps WASM community plugins because `PluginManager::
 * reload_plugin` rebuilds the whole sandbox from scratch (no live state
 * to preserve, unlike a script plugin's in-realm DOM/subscriptions).
 *
 * Runs the same install-time consent flow `main.tsx`'s boot path does,
 * scoped to the newly-discovered manifests only — a plugin already
 * running never re-prompts.
 */
export async function rescanCommunityPlugins(): Promise<RescanResult> {
  const host = getHost()
  const reg = getRegistry()
  if (!host || !reg) {
    return { manifests: [], added: [], denied: [], errors: [{ id: '', error: 'Shell is not booted yet' }] }
  }

  const manifests = await scanCommunityPlugins()
  reg.updateService('communityPluginManifests', manifests)

  const known = new Set(host.listAll().map(({ id }) => id))
  const newManifests = manifests.filter((m) => m.enabled && !known.has(m.id))
  if (newManifests.length === 0) {
    return { manifests, added: [], denied: [], errors: [] }
  }

  let denied = new Set<string>()
  try {
    const consent = await runInstallTimeConsent(newManifests)
    denied = consent.denied
  } catch (err) {
    clientLogger.warn('[CommunityLoader] rescan consent flow failed; continuing:', err)
  }
  const approved = newManifests.filter((m) => !denied.has(m.id))

  const errors: Array<{ id: string; error: string }> = []
  if (approved.length > 0) {
    const orchestrator = reg.hasService('sandboxOrchestrator')
      ? reg.getService<SandboxOrchestrator>('sandboxOrchestrator')
      : undefined
    // Re-blob the runtime source per rescan rather than threading
    // main.tsx's cached blob URL through — rescanning is a rare, manual
    // action; the extra Blob allocation is not worth the coupling.
    // Dynamic import (not a static top-level one): `virtual:sandbox-
    // runtime` is a Vite-only virtual module — a static import breaks
    // this file's plain-node unit tests (communityPluginLoader.test.ts),
    // which never call this function and so never need it resolved.
    let runtimeUrl: string | null = null
    const getRuntimeUrl = async () => {
      if (!runtimeUrl) {
        const { default: sandboxRuntimeSource } = await import('virtual:sandbox-runtime')
        runtimeUrl = URL.createObjectURL(
          new Blob([sandboxRuntimeSource], { type: 'application/javascript' }),
        )
      }
      return runtimeUrl
    }

    const loaded = await loadEnabledCommunityPlugins(approved, {
      orchestrator,
      getRuntimeUrl,
    })
    if (loaded.length > 0) {
      try {
        await host.loadAll(loaded)
      } catch (err) {
        errors.push({
          id: loaded.map((p) => p.manifest.id).join(', '),
          error: err instanceof Error ? err.message : String(err),
        })
      }
    }
  }

  const added = approved
    .filter((m) => host.getState(m.id) !== undefined && host.getState(m.id) !== 'error')
    .map((m) => m.id)
  for (const m of approved) {
    if (!added.includes(m.id) && host.getState(m.id) === 'error') {
      errors.push({ id: m.id, error: host.getError(m.id)?.message ?? 'Activation failed' })
    }
  }

  clientLogger.info(
    `[CommunityLoader] rescan: ${added.length} added, ${denied.size} denied, ${errors.length} failed`,
  )
  eventBus.emit(PLUGIN_LIST_CHANGED_EVENT, null)

  return { manifests, added, denied: [...denied], errors }
}

// ── Internal ──────────────────────────────────────────────────────────────────

async function loadOnePlugin(
  manifest: CommunityPluginManifest,
  options: LoadCommunityPluginsOptions,
): Promise<Plugin | null> {
  // Issue #86. `assertValidPluginId` (in PluginAPI.ts) forbids `:`
  // because it's the separator inside `plugin:<id>:<key>`
  // localStorage keys — a plugin id like `"a:b"` would otherwise
  // collide with the `a` plugin's namespace. The check fires
  // implicitly later when `apiFactory(pluginId)` runs, but by then
  // the bad id has already been registered, persisted, and
  // referenced in event subscriptions. Rejecting at manifest-parse
  // time means a malformed id never enters the registry.
  assertValidPluginId(manifest.id)

  // WI-33: reject incompatible plugins BEFORE touching their JS bundle.
  // Throwing here surfaces as a rejected Promise in
  // `loadEnabledCommunityPlugins`, which already logs + skips the plugin.
  // The Rust scanner surfaces the same `apiVersion` field to the settings
  // UI so the user sees an "Incompatible" chip without needing to dig
  // into the dev console.
  const verdict = checkApiVersion(manifest.id, manifest.apiVersion)
  if (!verdict.ok) throw verdict.error

  // WI-30e: sandboxed plugins route through the iframe orchestrator
  // instead of the shell-realm dynamic-import path. The orchestrator
  // spawns a null-origin iframe carrying the plugin bundle + the
  // `bootstrapSandboxedPlugin` runtime, completes the protocol
  // handshake, and installs host-side effects (commands, panels,
  // notifications) via the SandboxRouter. No `Plugin` is returned
  // because the ExtensionHost lifecycle doesn't apply to sandboxed
  // instances — the orchestrator owns load/unload.
  if (manifest.sandboxed === true) {
    return await loadSandboxedPlugin(manifest, options)
  }

  const mainPath = `${manifest.dir}/${manifest.main}`.replace(/\\/g, '/')

  // Read the JS source via the Tauri fs plugin
  let source: string
  try {
    source = await readTextFile(mainPath)
  } catch (err) {
    throw new Error(`Cannot read ${mainPath}: ${err}`)
  }

  // Wrap in a Blob URL so import() works without needing the file
  // to be served.  Note: relative imports inside the bundle will fail —
  // community plugins must be self-contained (bundled with a tool like Vite).
  const blob = new Blob([source], { type: 'application/javascript' })
  const url  = URL.createObjectURL(blob)

  try {
    const mod = await import(/* @vite-ignore */ url)
    const plugin: Plugin | undefined = mod.default ?? mod.plugin

    if (!plugin?.manifest?.id) {
      throw new Error(
        `${manifest.id}: module default export is not a valid Plugin object`
      )
    }

    // The plugin.json is authoritative for id/name/version — overwrite so
    // a plugin can't accidentally claim a core plugin's id via its bundle.
    plugin.manifest.id      = manifest.id
    plugin.manifest.name    = manifest.name
    plugin.manifest.version = manifest.version
    plugin.manifest.core    = false

    clientLogger.info(`[CommunityLoader] ✓ loaded ${manifest.id}`)
    return plugin
  } finally {
    URL.revokeObjectURL(url)
  }
}

async function loadSandboxedPlugin(
  manifest: CommunityPluginManifest,
  options: LoadCommunityPluginsOptions,
): Promise<null> {
  if (!options.orchestrator || !options.getRuntimeUrl) {
    clientLogger.warn(
      `[CommunityLoader] skipping sandboxed plugin '${manifest.id}' — ` +
      `caller did not provide a SandboxOrchestrator + getRuntimeUrl factory`,
    )
    return null
  }

  const mainPath = `${manifest.dir}/${manifest.main}`.replace(/\\/g, '/')

  // Read the bundle and wrap it in a blob: URL so the iframe's srcdoc
  // can dynamic-import it. The iframe runs at a null origin, so the
  // blob URL is reachable under the iframe's CSP (which explicitly
  // allows `blob:` for script-src — see buildSandboxSrcDoc).
  let source: string
  try {
    source = await readTextFile(mainPath)
  } catch (err) {
    throw new Error(`Cannot read ${mainPath}: ${err}`)
  }
  const blob = new Blob([source], { type: 'application/javascript' })
  const bundleUrl = URL.createObjectURL(blob)

  // The runtime bootstrap is shared across every sandboxed plugin. We
  // ask the caller to cache it and hand us back a URL — the default
  // wiring in main.tsx builds a blob URL once on boot and reuses it.
  const runtimeUrl = await options.getRuntimeUrl()

  const caps = parseManifestCapabilities(manifest.capabilities) ?? []

  try {
    await options.orchestrator.load({
      pluginId: manifest.id,
      bundleUrl,
      runtimeUrl,
      capabilities: new Set<string>(caps),
      manifestApiVersion: manifest.apiVersion,
    })
    clientLogger.info(`[CommunityLoader] ✓ loaded (sandboxed) ${manifest.id}`)
  } catch (err) {
    // Revoke the blob URL so the iframe's failure doesn't leak the
    // Blob reference. (The orchestrator revokes on successful load
    // via the srcdoc lifecycle — but a handshake failure shortcuts
    // that path.)
    try { URL.revokeObjectURL(bundleUrl) } catch { /* swallow */ }
    throw err
  }
  return null
}
