// Module-scoped singleton holding the PluginAPI handed to the
// pluginsMgmt plugin's `activate`. Mirrors nexus.commandPalette's
// paletteRuntime — keeps the React view free of prop-drilling for
// `commands.execute` / `internal.registry` lookups.

import type { PluginAPI } from '../../../types/plugin'

let _api: PluginAPI | null = null

export function setApi(api: PluginAPI) {
  _api = api
}

export function getApi(): PluginAPI {
  if (!_api) {
    throw new Error('[nexus.pluginsMgmt] PluginAPI not initialised yet')
  }
  return _api
}
