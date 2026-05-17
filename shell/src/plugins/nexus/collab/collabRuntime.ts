import type { PluginAPI } from '../../../types/plugin'

let _api: PluginAPI | null = null

export function setCollabApi(api: PluginAPI): void {
  _api = api
}

export function getCollabApi(): PluginAPI {
  if (!_api) throw new Error('[nexus.collab] api accessed before activate')
  return _api
}
