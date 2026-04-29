// shell/src/plugins/nexus/recall/recallApi.ts
//
// BL-044 — module-scope handle to the plugin's PluginAPI so the
// overlay component (which is rendered by the slot system without
// receiving the API as a prop) can reach `kernel.invoke` and
// `configuration.getValue`. Mirrors `cmdIApi.ts`.

import type { PluginAPI } from '../../../types/plugin'

let _api: PluginAPI | null = null

export function setRecallApi(api: PluginAPI): void {
  _api = api
}

export function getRecallApi(): PluginAPI {
  if (!_api) {
    throw new Error(
      '[nexus.recall] PluginAPI not yet bound — getRecallApi called before activate.',
    )
  }
  return _api
}
