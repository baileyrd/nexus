import type { PluginAPI } from '../../../types/plugin'

let _api: PluginAPI | null = null

export function setPickerApi(api: PluginAPI): void {
  _api = api
}

export function getPickerApi(): PluginAPI {
  if (!_api) throw new Error('[nexus.themePicker] api accessed before activate')
  return _api
}
