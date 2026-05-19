import { create } from 'zustand'
import type { Capability } from '@nexus/extension-api'

/**
 * State for the Plugins modal.
 *
 * Rows come from two internal services registered on the PluginRegistry:
 *   - `pluginList`                — built-in nexus.* / core.* plugins
 *   - `communityPluginManifests`  — drop-folder plugins from ~/.nexus-shell/plugins/
 *
 * They're tagged with a discriminant `kind` at read time so the view can
 * render two different row shapes without a second lookup.
 *
 * `capabilities` is `null` when the manifest field is absent (rendered as
 * "(unknown)") and `[]` when the plugin explicitly declares no
 * capabilities (rendered as "(none)"). See `capabilityInfo.ts` for the
 * risk bucketing and the `parseManifestCapabilities` helper that
 * normalises the raw manifest value into this shape.
 */

export interface BuiltInPluginRow {
  kind: 'builtin'
  id: string
  name: string
  version: string
  core: boolean
  state: string
  error?: string
  description?: string
  capabilities: Capability[] | null
  /**
   * True when the plugin contributes either a `settingsTabs` entry or a
   * `configuration` schema — i.e. the `nexus.plugins.configure` command
   * has a destination to deep-link to. Computed by `readRows` against
   * the live registries.
   */
  canConfigure?: boolean
  /**
   * True when the plugin is in `DEFAULT_OFF_PLUGINS` — i.e. the user can
   * mid-session disable it via `disableBuiltinPlugin` without breaking
   * the shell. Required built-ins (default-on) render the toggle as
   * disabled.
   */
  optional?: boolean
}

export interface CommunityPluginRow {
  kind: 'community'
  id: string
  name: string
  version: string
  enabled: boolean
  description?: string
  author?: string
  dir: string
  manifestPath: string
  capabilities: Capability[] | null
  /**
   * WI-33 — populated when the plugin's declared `apiVersion` does not
   * match the shell's `PLUGIN_API_VERSION`. When present, the row
   * renders an "Incompatible" badge and the enable-toggle is disabled.
   */
  incompatible?: {
    requested: number
    supported: number
  }
  /**
   * WI-31 — summary of install-time capability grants.
   *
   *   - `declared` : total caps declared in plugin.json (PascalCase,
   *                  after `parseManifestCapabilities` filtering).
   *                  Null when the manifest omits `capabilities`.
   *   - `granted`  : count of HIGH-risk caps the user has consented to
   *                  (from `<plugin_dir>/granted_caps.json`). Low /
   *                  medium caps are auto-granted and aren't part of
   *                  the "granted" tally.
   *   - `denied`   : user denied this plugin's consent prompt this
   *                  session; the plugin is skipped at activation.
   *
   * Used to render the "Granted N/M" subtitle and the "Review
   * capabilities" button in both the PluginsMgmt modal and the
   * Settings > Plugins tab.
   */
  grantSummary?: {
    declared: number | null
    granted: number
    denied: boolean
  }
  /** Absolute path to the plugin's directory (for consent write-back). */
  pluginDir?: string
  /** See `BuiltInPluginRow.canConfigure`. */
  canConfigure?: boolean
}

/**
 * WI-43: a plugin shipped in the shell binary but default-off. Rendered
 * in a dedicated "Available (disabled)" section with a one-click
 * Enable button. Enabling writes the id into the `plugins.enabled`
 * config key and (since there is no in-session activate path yet)
 * prompts the user to reload to pick up the new registration.
 */
export interface AvailablePluginRow {
  kind: 'available'
  id: string
  name: string
  version: string
  core: boolean
  description?: string
}

export type PluginRow =
  | BuiltInPluginRow
  | CommunityPluginRow
  | AvailablePluginRow

export interface PluginsMgmtState {
  visible: boolean
  query: string
  rows: PluginRow[]

  open(): void
  close(): void
  setQuery(q: string): void
  setRows(rs: PluginRow[]): void
  /** Flip just one community row's enabled flag — used for optimistic UI. */
  updateCommunityEnabled(id: string, enabled: boolean): void
}

export const usePluginsMgmtStore = create<PluginsMgmtState>((set) => ({
  visible: false,
  query: '',
  rows: [],

  open: () => set({ visible: true, query: '' }),
  close: () => set({ visible: false }),

  setQuery: (q) => set({ query: q }),
  setRows: (rs) => set({ rows: rs }),

  updateCommunityEnabled: (id, enabled) =>
    set((s) => ({
      rows: s.rows.map((r) =>
        r.kind === 'community' && r.id === id ? { ...r, enabled } : r,
      ),
    })),
}))
