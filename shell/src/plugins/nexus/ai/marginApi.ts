// shell/src/plugins/nexus/ai/marginApi.ts
//
// BL-036 phase 4 — module-scoped PluginAPI handle for the
// margin-suggestion idle-trigger extension. Mirrors `ghostApi.ts`
// (BL-034) and `cmdIApi.ts` (BL-032): the editor plugin's CM
// extension factories run far from the AI plugin's activate closure,
// so they need a lazily-resolved handle to reach
// `api.kernel.invoke('com.nexus.ai', 'stream_chat', …)` via
// `requestPass`.
//
// `getMarginApi()` returns `null` (not throws) when the AI plugin
// hasn't activated yet, so the editor can mount before the AI plugin
// without blowing up — the trigger short-circuits and a future doc
// edit will retry once the handle lands.

import type { PluginAPI } from '../../../types/plugin'

let _api: PluginAPI | null = null

export function setMarginApi(api: PluginAPI): void {
  _api = api
}

export function getMarginApi(): PluginAPI | null {
  return _api
}

/** Test-only — drop the cached handle so each test starts clean. */
export function _resetMarginApiForTests(): void {
  _api = null
}
