// C84 (#437) — controls visibility of the per-plugin audit-log overlay
// opened from a Plugins-modal row. Kept separate from
// `pluginsMgmtStore` since it's purely view-local UI state (which
// plugin's audit timeline is currently open), not plugin-list data.

import { create } from 'zustand'

export interface AuditLogState {
  /** The plugin id whose audit timeline is open, or `null` when closed. */
  pluginId: string | null
  open(pluginId: string): void
  close(): void
}

export const useAuditLogStore = create<AuditLogState>((set) => ({
  pluginId: null,
  open: (pluginId) => set({ pluginId }),
  close: () => set({ pluginId: null }),
}))
