// Module-scoped PluginAPI holder for the files plugin. Same pattern
// as nexus.titleBar/runtime.ts — the React component fires actions
// (new file / new folder prompts) that need `api.input.prompt`, and
// threading the api through context for one consumer is heavier than
// it's worth. Set once by the plugin's `activate`, read on demand by
// the tree component.

import type { PluginAPI } from '../../../types/plugin'

let _api: PluginAPI | null = null

export function setApi(api: PluginAPI) {
  _api = api
}

export function getApi(): PluginAPI | null {
  return _api
}
