// Thin wrappers over the com.nexus.storage canvas_* IPC handlers
// added in crates/nexus-storage/src/core_plugin.rs (ids 35–39). Kept
// here as a plugin-local module so every callsite type-checks against
// the same minimal shape.

import type { PluginAPI } from '../../../types/plugin'

export const STORAGE_PLUGIN_ID = 'com.nexus.storage'

/** Subset of CanvasFile the Phase-1 surface needs. Full renderer will
 *  widen this as it adopts more node / edge fields. */
export interface CanvasDoc {
  version: string
  nodes: unknown[]
  edges: unknown[]
}

export interface CanvasKernelClient {
  read(relpath: string): Promise<CanvasDoc>
}

export function makeCanvasKernelClient(kernel: PluginAPI['kernel']): CanvasKernelClient {
  return {
    async read(relpath) {
      return kernel.invoke<CanvasDoc>(STORAGE_PLUGIN_ID, 'canvas_read', { path: relpath })
    },
  }
}
