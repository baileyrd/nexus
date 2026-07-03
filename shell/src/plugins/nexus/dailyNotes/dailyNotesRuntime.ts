// Module-scoped singleton holding the PluginAPI handed to the
// dailyNotes plugin's `activate`. Mirrors commandPalette/paletteRuntime.ts.

import type { PluginAPI } from '../../../types/plugin'

let _api: PluginAPI | null = null

export function setApi(api: PluginAPI): void {
  _api = api
}

export function getApi(): PluginAPI {
  if (!_api) {
    throw new Error('[nexus.dailyNotes] PluginAPI not initialised yet')
  }
  return _api
}
