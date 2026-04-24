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
      console.warn(
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
    console.warn('[CommunityLoader] scan_plugin_directory failed:', (results[0] as PromiseRejectedResult).reason)
  }
  if (results[1].status === 'rejected') {
    console.warn('[CommunityLoader] scan_plugin_directory_at failed:', (results[1] as PromiseRejectedResult).reason)
  }

  // Merge: installed plugins take precedence; repo fills in anything not installed
  const installedIds = new Set(installed.map(m => m.id))
  const merged = [
    ...installed,
    ...fromRepo
      .filter(m => !installedIds.has(m.id))
      .map(m => ({ ...m, _source: 'repo' as const })),
  ]

  console.info(
    `[CommunityLoader] ${merged.length} plugin(s) discovered` +
    (import.meta.env.DEV ? ` (${fromRepo.length} from repo)` : '')
  )
  return merged
}

/**
 * Load all *enabled* community plugins.
 * Reads each JS bundle via the fs plugin (so the file doesn't need to be
 * served), wraps it in a Blob URL, then dynamic-imports it.
 *
 * The JS bundle must export a Plugin as default:
 *   export default { manifest, activate, deactivate }
 */
export async function loadEnabledCommunityPlugins(
  manifests: CommunityPluginManifest[]
): Promise<Plugin[]> {
  const enabled = manifests.filter(m => m.enabled)
  if (enabled.length === 0) return []

  const results = await Promise.allSettled(
    enabled.map(m => loadOnePlugin(m))
  )

  const plugins: Plugin[] = []
  for (const [i, result] of results.entries()) {
    if (result.status === 'fulfilled' && result.value) {
      plugins.push(result.value)
    } else if (result.status === 'rejected') {
      console.error(
        `[CommunityLoader] ✗ ${enabled[i].id}:`,
        result.reason
      )
    }
  }

  return plugins
}

// ── Internal ──────────────────────────────────────────────────────────────────

async function loadOnePlugin(
  manifest: CommunityPluginManifest
): Promise<Plugin | null> {
  // WI-33: reject incompatible plugins BEFORE touching their JS bundle.
  // Throwing here surfaces as a rejected Promise in
  // `loadEnabledCommunityPlugins`, which already logs + skips the plugin.
  // The Rust scanner surfaces the same `apiVersion` field to the settings
  // UI so the user sees an "Incompatible" chip without needing to dig
  // into the dev console.
  const verdict = checkApiVersion(manifest.id, manifest.apiVersion)
  if (!verdict.ok) throw verdict.error

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

    console.info(`[CommunityLoader] ✓ loaded ${manifest.id}`)
    return plugin
  } finally {
    URL.revokeObjectURL(url)
  }
}
