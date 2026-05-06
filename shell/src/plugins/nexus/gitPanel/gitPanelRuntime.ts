import type { PluginAPI } from '../../../types/plugin'

let _api: PluginAPI | null = null

export function setGitPanelApi(api: PluginAPI): void {
  _api = api
}

export function getGitPanelApi(): PluginAPI {
  if (!_api) throw new Error('[nexus.gitPanel] api accessed before activate')
  return _api
}
