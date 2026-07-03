// Module-scoped singleton holding the PluginAPI handed to the
// taskDashboard plugin's `activate`. Mirrors commandPalette/paletteRuntime.ts
// — keeps TaskDashboardView free of prop-drilling for `kernel.invoke` /
// `events.emit`.

import type { PluginAPI } from '../../../types/plugin'

let _api: PluginAPI | null = null

export function setApi(api: PluginAPI): void {
  _api = api
}

export function getApi(): PluginAPI {
  if (!_api) {
    throw new Error('[nexus.taskDashboard] PluginAPI not initialised yet')
  }
  return _api
}
