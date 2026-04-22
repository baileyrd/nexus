// Module-scoped runtime holder exposing editor operations that close
// over the plugin's activated PluginAPI (confirmAndClose wraps the
// confirm modal + config lookup). Same pattern as files/runtime.ts,
// but the exported value is the operation bundle rather than the raw
// api — the ops already carry the api through closure.

import type { EditorView as CMEditorView } from '@codemirror/view'
import type { EditorKernelClient } from './kernelClient.ts'
import type { SessionManager } from './sessionManager.ts'

export interface EditorRuntime {
  confirmAndClose: (relpath: string) => Promise<void>
  openUntitled: () => void
  closeAll: () => Promise<void>
  /**
   * Typed editor-kernel client. Threaded through the runtime so the
   * `EditorView` (a generic React component that can't take a
   * plugin-API dep) can hand it to the Phase 5 transaction bridge.
   */
  kernelClient: EditorKernelClient
  /**
   * Session manager used by the bridge to resolve the current block
   * tree (specifically `tree.root_blocks[0]` for v1 coarse mapping).
   */
  sessionManager: SessionManager
  /**
   * Error sink for bridge failures — wired to `api.notifications.show`
   * by the plugin's activate(). Absent in test drivers; callers fall
   * back to `console.error`.
   */
  reportBridgeError?: (message: string, err: unknown) => void
}

let _runtime: EditorRuntime | null = null

export function setEditorRuntime(runtime: EditorRuntime) {
  _runtime = runtime
}

export function getEditorRuntime(): EditorRuntime | null {
  return _runtime
}

// Active CodeMirror view registry. The Find/Replace commands in
// `index.ts` need to call `openSearchPanel(view)` on whichever CM
// view the user is currently editing. EditorView mounts/unmounts
// register the active view here so the command can resolve it
// without taking a React dep.
let _activeCmView: CMEditorView | null = null

export function setActiveCmView(view: CMEditorView | null) {
  _activeCmView = view
}

export function getActiveCmView(): CMEditorView | null {
  return _activeCmView
}

