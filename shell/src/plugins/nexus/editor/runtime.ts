// Module-scoped runtime holder exposing editor operations that close
// over the plugin's activated PluginAPI (confirmAndClose wraps the
// confirm modal + config lookup). Same pattern as files/runtime.ts,
// but the exported value is the operation bundle rather than the raw
// api — the ops already carry the api through closure.

import type { EditorView as CMEditorView } from '@codemirror/view'
import type { EditorKernelClient } from './kernelClient.ts'
import type { EditorKeybindings } from './cm/extensions.ts'
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
  /**
   * Kernel-event subscriber threaded through to the BL-012 split-4
   * database-view decoration watcher. `KernelAPI.on(prefix, h)` —
   * the watcher uses the storage `file_*` topics to invalidate the
   * inline-grid cache on external `.bases` edits.
   */
  kernelEvents?: import('./cm/databaseViewDecorations.ts').KernelEventSubscriber
  /**
   * Click handler for inline `[[<file>#^<uuid>]]` block links
   * (BL-049 phase 2). When set, the live-preview extension stack
   * mounts the navigation extension; activate-time wires this to
   * an `events.emit('files:open', …)` followed by a
   * `nexus.editor:reveal-block` event so the receiving tab can
   * scroll to the target block once it finishes loading.
   */
  onBlockLinkNavigate?: (
    link: import('./blockLinks.ts').ParsedBlockLink,
  ) => void
  /**
   * BL-070: live read of the `nexus.editor.keybindings` setting. Read
   * at tab-render time so a setting flip + tab reopen picks up the
   * new value; live-mutating an open tab's keymap is out of scope.
   */
  getKeybindings: () => EditorKeybindings
  /**
   * BL-075: live read of the `nexus.editor.codeFileExtensions`
   * setting, parsed into a normalised array of lowercase extensions
   * with no leading dots and whitespace trimmed. Read at tab-render
   * time so a setting flip + tab reopen picks up the new value;
   * live-mutating an open tab's mode is out of scope.
   */
  getCodeFileExtensions: () => readonly string[]
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

