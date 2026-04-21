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
  /** Absolute path to the plugin's directory — injected by the Rust scanner */
  dir:          string
  /** Absolute path to plugin.json — injected by the Rust scanner */
  manifestPath: string
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
