// Holds a reference to the skills plugin's PluginAPI so deep React
// children (SkillsView delete confirm) can reach `api.input.confirm`
// without prop-drilling through every intermediate component. Mirrors
// nexus.bases/runtime.ts and nexus.files/runtime.ts.

import type { PluginAPI } from '../../../types/plugin'

let api: PluginAPI | null = null

export function setSkillsRuntime(nextApi: PluginAPI): void {
  api = nextApi
}

export function getSkillsApi(): PluginAPI | null {
  return api
}
