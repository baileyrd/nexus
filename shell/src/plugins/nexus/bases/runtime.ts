// Holds references the bases plugin populates during activate so
// modules that aren't plugins (NewBaseDialog, a future schema
// editor) can reach the kernel client and the plugin API without
// prop-drilling. Mirrors nexus.files/runtime.ts.

import type { PluginAPI } from '../../../types/plugin'
import type { BasesKernelClient } from './kernelClient'

let api: PluginAPI | null = null
let client: BasesKernelClient | null = null

export function setRuntime(nextApi: PluginAPI, nextClient: BasesKernelClient): void {
  api = nextApi
  client = nextClient
}

export function getBasesApi(): PluginAPI | null {
  return api
}

export function getBasesClient(): BasesKernelClient | null {
  return client
}
