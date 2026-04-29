// shell/src/plugins/nexus/enrich/index.ts
//
// BL-045 — MEM auto-enrichment on save.
//
// Plugin scaffold + runtime wiring. The plugin is shipped DEFAULT-OFF
// (catalog: DEFAULT_OFF_PLUGINS) — users opt in via Settings →
// Plugins. When enabled it:
//
//   1. Subscribes to `files:saved` (emitted by the editor save command).
//   2. Throttles per-file (5 s) to coalesce rapid consecutive saves.
//   3. Calls `com.nexus.ai::enrich_file` (HANDLER_ENRICH_FILE = 15)
//      to get tags + summary + related notes.
//   4. Surfaces the proposal in an inline accept-gate panel.
//   5. On Accept, calls `com.nexus.ai::enrich_apply`
//      (HANDLER_ENRICH_APPLY = 16) which merges the proposal into
//      YAML frontmatter (with a body-hash drift guard).
//
// Enrichment never blocks the save itself — failures are surfaced in
// the panel but never thrown.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { attachRuntime } from './enrichRuntime'
import { EnrichAcceptGate } from './EnrichAcceptGate'
import { setEnrichApi } from './enrichApi'

const VIEW_ID_GATE = 'nexus.enrich.gate'

export const enrichPlugin: Plugin = {
  manifest: {
    id: 'nexus.enrich',
    name: 'AI Auto-Enrichment',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    contributes: {
      configuration: {
        pluginId: 'nexus.enrich',
        title: 'AI Auto-Enrichment',
        order: 72,
        schema: [],
      },
    },
  },

  activate(api: PluginAPI) {
    api.configuration.register(enrichPlugin.manifest.contributes!.configuration!)
    setEnrichApi(api)
    attachRuntime(api)
    api.views.register(VIEW_ID_GATE, {
      slot: 'overlay',
      // Sit above the recall overlay (26) so a stacked open keeps the
      // accept-gate visible. The gate is small and unobtrusive but it
      // also represents a confirmation step the user shouldn't miss.
      priority: 27,
      component: EnrichAcceptGate,
    })
  },
}
