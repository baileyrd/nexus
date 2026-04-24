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
  capabilities: Capability[] | null
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
}

export type PluginRow = BuiltInPluginRow | CommunityPluginRow

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
