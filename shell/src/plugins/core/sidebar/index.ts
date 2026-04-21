// src/plugins/core/sidebar/index.ts
//
// Phase 7 legacy stub. The original core.sidebar plugin registered a
// SidebarView into slot:'sidebar', which was removed when the left
// sidedock became a workspace sidedock. This file is retained as a
// stub so any build that still imports it from the template compiles.
// It is NOT loaded by main.tsx.

import type { Plugin } from '../../../types/plugin'

export const sidebarPlugin: Plugin = {
  manifest: {
    id: 'core.sidebar',
    name: 'Sidebar (legacy stub)',
    version: '1.0.0',
    core: true,
    activationEvents: [],
    contributes: {},
  },
  activate() {
    // no-op
  },
}
