// Module-scoped runtime holder exposing editor operations that close
// over the plugin's activated PluginAPI (confirmAndClose wraps the
// confirm modal + config lookup). Same pattern as files/runtime.ts,
// but the exported value is the operation bundle rather than the raw
// api — the ops already carry the api through closure.

export interface EditorRuntime {
  confirmAndClose: (relpath: string) => Promise<void>
  openUntitled: () => void
  closeAll: () => Promise<void>
}

let _runtime: EditorRuntime | null = null

export function setEditorRuntime(runtime: EditorRuntime) {
  _runtime = runtime
}

export function getEditorRuntime(): EditorRuntime | null {
  return _runtime
}
