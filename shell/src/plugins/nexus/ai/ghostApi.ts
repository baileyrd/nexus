// shell/src/plugins/nexus/ai/ghostApi.ts
//
// BL-034 — module-scoped PluginAPI handle for the CodeMirror ghost
// completion extension. Mirrors `cmdIApi.ts` 1:1 because the editor
// plugin's CM extension factories run far from the AI plugin's
// activate closure — they need a stable, lazily-resolved handle to
// reach `api.kernel.invoke('com.nexus.ai', 'stream_chat', …)`.
//
// `getGhostApi()` returns `null` (not throws) when the AI plugin
// hasn't activated yet, so the editor can mount before the AI plugin
// without blowing up — ghost suggestions just stay dark until the
// next typing event after the AI plugin comes online.

import type { PluginAPI } from '../../../types/plugin'

let _api: PluginAPI | null = null

export function setGhostApi(api: PluginAPI): void {
  _api = api
}

export function getGhostApi(): PluginAPI | null {
  return _api
}

/** Test-only — drop the cached handle so each test starts clean. */
export function _resetGhostApiForTests(): void {
  _api = null
}
