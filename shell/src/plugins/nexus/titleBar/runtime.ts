// Module-scoped PluginAPI handle for the title-bar component. Same
// pattern as nexus.commandPalette's paletteRuntime — the React
// component fires actions (open workspace, focus search) that need
// `api.commands.execute`, and threading the api through a context
// provider for one consumer would be heavy.
//
// Set once by the title-bar plugin's `activate`, read on demand by
// the React component below it.

import type { PluginAPI } from '../../../types/plugin'

let _api: PluginAPI | null = null

export function setApi(api: PluginAPI) {
  _api = api
}

export function getApi(): PluginAPI | null {
  return _api
}
