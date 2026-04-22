// Thin wrappers over the com.nexus.storage canvas_* IPC handlers
// added in crates/nexus-storage/src/core_plugin.rs (ids 35–39). The
// shapes below mirror nexus_formats::canvas::types; fields we don't
// yet render are kept typed-but-unused so future phases don't have to
// widen the interface again.

import type { PluginAPI } from '../../../types/plugin'

export const STORAGE_PLUGIN_ID = 'com.nexus.storage'
export const LINKPREVIEW_PLUGIN_ID = 'com.nexus.linkpreview'
export const TERMINAL_PLUGIN_ID = 'com.nexus.terminal'

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

/** Minimal subset of `nexus_types::bases::Base` that the canvas
 *  database-node overlay actually reads. The kernel returns the
 *  full Base; we type only what the mini-grid needs so future
 *  additions (views, relations, metadata) can extend this
 *  interface without widening every consumer. */
export interface BaseSummary {
  name: string
  schema: {
    version?: string
    /** Field name → field definition. Definitions are opaque to the
     *  shell (they're editor-specific config); we only use the keys
     *  to order columns. */
    fields: Record<string, unknown>
  }
  records: Array<{
    id: string
    /** All non-id record fields flatten into the same object on the
     *  wire thanks to `#[serde(flatten)]`, so anything that isn't
     *  `id` is a user-defined field value. */
    [field: string]: unknown
  }>
}

/** Shape returned by com.nexus.linkpreview::fetch. Every metadata
 *  field is nullable — the shell renders whatever it gets and falls
 *  back to the raw URL when everything is missing. Mirrors
 *  `nexus_linkpreview::LinkPreview`. */
export interface LinkPreview {
  url: string
  title?: string | null
  description?: string | null
  image_url?: string | null
  site_name?: string | null
  favicon_url?: string | null
}

export interface CanvasKernelClient {
  read(relpath: string): Promise<CanvasDoc>
  patch(relpath: string, ops: CanvasPatchOp[]): Promise<void>
  /** Fetch preview metadata for an external URL. Resolves with a
   *  best-effort preview (mostly-empty on transport failure) or
   *  rejects if the URL itself is invalid. */
  fetchLinkPreview(url: string): Promise<LinkPreview>
  /** Read an arbitrary file's bytes out of the forge. Returns the
   *  raw byte array (same shape as `com.nexus.storage::read_file`),
   *  or null if the file doesn't exist. The file-node overlay uses
   *  this to embed previews of linked `.md` / image / text files. */
  readFile(relpath: string): Promise<Uint8Array | null>
  /** Load a `.bases` directory's full contents (schema + records).
   *  Used by the database-node overlay to render a mini-grid. */
  loadBase(relpath: string): Promise<BaseSummary>
  /** Start a fresh PTY session. Returns the session id — callers
   *  hand it to `sendInput` / `readRawSince` / `closeSession`. Used
   *  by the terminal-node overlay's Run button. */
  createTerminalSession(): Promise<string>
  /** Send a line of input to a session. `send_input` automatically
   *  appends a newline, which is what we want for "run one command". */
  sendTerminalInput(sessionId: string, line: string): Promise<void>
  /** Drain raw PTY bytes past `cursor`. Returns the bytes plus the
   *  advanced cursor for the next call. */
  readTerminalRaw(
    sessionId: string,
    cursor: number,
  ): Promise<{ cursor: number; bytes: Uint8Array }>
  /** Tear a session down. Safe to call multiple times — the kernel
   *  treats unknown ids as no-ops for the overlay's cleanup path. */
  closeTerminalSession(sessionId: string): Promise<void>
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
    async fetchLinkPreview(url) {
      return kernel.invoke<LinkPreview>(LINKPREVIEW_PLUGIN_ID, 'fetch', { url })
    },
    async readFile(relpath) {
      const resp = await kernel.invoke<{ bytes: number[] | null }>(
        STORAGE_PLUGIN_ID,
        'read_file',
        { path: relpath },
      )
      return resp.bytes == null ? null : Uint8Array.from(resp.bytes)
    },
    async loadBase(relpath) {
      return kernel.invoke<BaseSummary>(STORAGE_PLUGIN_ID, 'base_load', {
        path: relpath,
      })
    },
    async createTerminalSession() {
      const resp = await kernel.invoke<{ id: string }>(
        TERMINAL_PLUGIN_ID,
        'create_session',
        {},
      )
      return resp.id
    },
    async sendTerminalInput(sessionId, line) {
      await kernel.invoke<unknown>(TERMINAL_PLUGIN_ID, 'send_input', {
        id: sessionId,
        input: line,
      })
    },
    async readTerminalRaw(sessionId, cursor) {
      const resp = await kernel.invoke<{ cursor: number; data: number[] }>(
        TERMINAL_PLUGIN_ID,
        'read_raw_since',
        { id: sessionId, cursor },
      )
      return {
        cursor: resp.cursor,
        bytes: resp.data.length ? Uint8Array.from(resp.data) : new Uint8Array(),
      }
    },
    async closeTerminalSession(sessionId) {
      await kernel
        .invoke<unknown>(TERMINAL_PLUGIN_ID, 'close_session', { id: sessionId })
        .catch(() => {
          // Session already gone — canvas teardown is idempotent.
        })
    },
  }
}
