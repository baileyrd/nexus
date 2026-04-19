// Module-scoped singleton holding the PluginAPI handed to the
// commandPalette plugin's `activate`. Mirrors the pattern used by
// nexus.files (see ./files/kernelClient.ts) — keeps the React
// component free of prop-drilling for `commands.execute` /
// `commands.all` without spinning up a context provider just for
// one consumer.
//
// Set once in `activate`, read by `CommandPalette` on demand.

import type { PluginAPI } from '../../../types/plugin'

let _api: PluginAPI | null = null

export function setApi(api: PluginAPI) {
  _api = api
}

export function getApi(): PluginAPI {
  if (!_api) {
    throw new Error('[nexus.commandPalette] PluginAPI not initialised yet')
  }
  return _api
}
