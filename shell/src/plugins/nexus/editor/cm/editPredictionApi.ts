// BL-139 — module-scoped PluginAPI handle for the CodeMirror
// edit-prediction extension. Mirrors `ghostApi.ts` for BL-034 and
// `cmdIApi.ts` — the editor plugin's CM extension factories run far
// from the AI plugin's activate closure, so they need a stable,
// lazily-resolved handle to reach
// `api.kernel.invoke('com.nexus.ai', 'predict', …)`.
//
// `getEditPredictionApi()` returns `null` (not throws) when the AI
// plugin hasn't activated yet — the editor can mount before the AI
// plugin without blowing up; predictions just stay dark until the
// next keystroke after the AI plugin comes online.

import type { PluginAPI } from '../../../../types/plugin'

let _api: PluginAPI | null = null

export function setEditPredictionApi(api: PluginAPI): void {
  _api = api
}

export function getEditPredictionApi(): PluginAPI | null {
  return _api
}

/** Test-only — drop the cached handle so each test starts clean. */
export function _resetEditPredictionApiForTests(): void {
  _api = null
}
