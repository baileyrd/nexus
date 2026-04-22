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

/**
 * Mirrors `nexus_storage::CanvasPatchOp` on the wire. Serde uses
 * `#[serde(tag = "op", rename_all = "snake_case")]` so the `op`
 * discriminator values must stay in sync with the Rust enum.
 */
export type CanvasPatchOp =
  | { op: 'node_add'; node: CanvasNode }
  | { op: 'node_remove'; id: string }
  | { op: 'node_move'; id: string; x: number; y: number }
  | { op: 'node_update'; node: CanvasNode }
  | { op: 'edge_add'; edge: CanvasEdge }
  | { op: 'edge_remove'; id: string }
  | { op: 'edge_update'; edge: CanvasEdge }

export interface CanvasKernelClient {
  read(relpath: string): Promise<CanvasDoc>
  patch(relpath: string, ops: CanvasPatchOp[]): Promise<void>
}

export function makeCanvasKernelClient(kernel: PluginAPI['kernel']): CanvasKernelClient {
  return {
    async read(relpath) {
      return kernel.invoke<CanvasDoc>(STORAGE_PLUGIN_ID, 'canvas_read', { path: relpath })
    },
    async patch(relpath, ops) {
      if (ops.length === 0) return
      await kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'canvas_patch', {
        path: relpath,
        ops,
      })
    },
  }
}
