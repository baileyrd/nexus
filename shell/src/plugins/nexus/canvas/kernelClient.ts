// Thin wrappers over the com.nexus.storage canvas_* IPC handlers
// added in crates/nexus-storage/src/core_plugin.rs (ids 35–39). The
// shapes below mirror nexus_formats::canvas::types; fields we don't
// yet render are kept typed-but-unused so future phases don't have to
// widen the interface again.

import type { PluginAPI } from '../../../types/plugin'

export const STORAGE_PLUGIN_ID = 'com.nexus.storage'

export type CanvasNodeType =
  | 'file'
  | 'text'
  | 'link'
  | 'group'
  | 'database'
  | 'terminal'

export type CanvasEdgeType = 'solid' | 'dashed' | 'dotted'

export interface CanvasNode {
  id: string
  type: CanvasNodeType
  x: number
  y: number
  width: number
  height: number
  color?: string
  label?: string
  collapsed?: boolean
  // Type-specific fields (Obsidian-compatible).
  file?: string
  text?: string
  url?: string
  source?: string
  command?: string
}

export interface CanvasEdge {
  id: string
  fromNode: string
  toNode: string
  type?: CanvasEdgeType
  label?: string
  color?: string
}

export interface CanvasDoc {
  version: string
  nodes: CanvasNode[]
  edges: CanvasEdge[]
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
