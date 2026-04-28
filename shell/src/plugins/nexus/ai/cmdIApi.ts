// shell/src/plugins/nexus/ai/cmdIApi.ts
//
// BL-032 — module-scoped PluginAPI handle for the Cmd+I overlay,
// mirroring `commandPalette/paletteRuntime.ts`. Set once during the AI
// plugin's `activate`; read by `CmdIOverlay` (which is rendered by the
// shell, not by the plugin module's closure scope).
//
// Held separately from `aiRuntime.ts`'s `kernel` singleton because the
// overlay can outlive a single chat-session lifecycle and we want a
// clear boundary between the two surfaces.

import type { PluginAPI } from '../../../types/plugin'

let _api: PluginAPI | null = null

export function setCmdIApi(api: PluginAPI): void {
  _api = api
}

export function getApi(): PluginAPI {
  if (!_api) {
    throw new Error('[nexus.ai/cmdI] PluginAPI not initialised yet')
  }
  return _api
}

/** Test-only — drop the cached handle so each test starts clean. */
export function _resetCmdIApiForTests(): void {
  _api = null
}
