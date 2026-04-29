// shell/src/plugins/nexus/enrich/enrichApi.ts
//
// BL-045 — module-scope handle to the plugin's PluginAPI so the
// EnrichAcceptGate component (rendered by the slot system without
// the API as a prop) can call `applyPending(api)` from a click
// handler. Mirrors `recallApi.ts`.

import type { PluginAPI } from '../../../types/plugin'

let _api: PluginAPI | null = null

export function setEnrichApi(api: PluginAPI): void {
  _api = api
}

export function getEnrichApi(): PluginAPI {
  if (!_api) {
    throw new Error(
      '[nexus.enrich] PluginAPI not yet bound — getEnrichApi called before activate.',
    )
  }
  return _api
}
