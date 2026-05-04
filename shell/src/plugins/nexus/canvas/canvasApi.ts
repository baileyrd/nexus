// Module-scope holder for the canvas plugin's PluginAPI handle. Set
// once in activate(), read by view-side code (rails, drag sources) that
// needs to surface toasts without threading the api prop through every
// component layer.

import type { PluginAPI } from '../../../types/plugin'

let api: PluginAPI | null = null

export function setCanvasApi(handle: PluginAPI): void {
  api = handle
}

export function getCanvasApi(): PluginAPI | null {
  return api
}
