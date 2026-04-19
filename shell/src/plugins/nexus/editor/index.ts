import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { EditorView } from './EditorView'
import { useEditorStore } from './editorStore'

const VIEW_ID = 'nexus.editor.view'
const EVENT_FILE_OPEN = 'files:open'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
// Verified against crates/nexus-storage/src/core_plugin.rs::dispatch:
//   HANDLER_READ_FILE args are `{ "path": String }`, response is
//   `{ "bytes": Vec<u8> }`. The arg key is `path`, NOT `relpath`
//   (unlike `list_dir` / `create_file`). The command name is mapped
//   in nexus-bootstrap/src/lib.rs.
const READ_FILE_COMMAND = 'read_file'

interface FileOpenPayload {
  relpath: string
  name: string
}

interface ReadFileResponse {
  /** Serde of `Vec<u8>` over JSON — arrives as a number[] of bytes. */
  bytes: number[]
}

/**
 * Decode a byte array response from `com.nexus.storage:read_file` as
 * UTF-8 text. Returns a human-readable sentinel for non-decodable
 * bytes so a binary file doesn't look like an error to the user.
 */
function decodeUtf8(bytes: number[]): string {
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(new Uint8Array(bytes))
  } catch {
    return '(binary or non-UTF-8 file)'
  }
}

export const editorPlugin: Plugin = {
  manifest: {
    id: 'nexus.editor',
    name: 'Editor',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    // Not strictly dependent on nexus.files — we listen to the local
    // `files:open` event bus and would render the empty state fine
    // without it. The dependency on workspace/sidebar keeps plugin
    // load order sensible (workspace → sidebar → files → editor).
    dependsOn: ['nexus.workspace', 'nexus.sidebar'],
    contributes: {},
  },

  async activate(api: PluginAPI) {
    // Remember the last file-open request so the Retry button can
    // re-invoke the same load without round-tripping through the
    // file tree again.
    let lastPayload: FileOpenPayload | null = null

    const loadFile = async (payload: FileOpenPayload) => {
      lastPayload = payload
      const store = useEditorStore.getState()
      store.setLoading(true)
      store.setError(null)
      try {
        const resp = await api.kernel.invoke<ReadFileResponse>(
          STORAGE_PLUGIN_ID,
          READ_FILE_COMMAND,
          { path: payload.relpath },
        )
        const content = decodeUtf8(resp.bytes ?? [])
        useEditorStore.getState().setFile({
          relpath: payload.relpath,
          name: payload.name,
          content,
        })
      } catch (err) {
        useEditorStore.getState().setError(String(err))
        useEditorStore.getState().setFile(null)
      } finally {
        useEditorStore.getState().setLoading(false)
      }
    }

    const handleRetry = () => {
      if (lastPayload) void loadFile(lastPayload)
    }

    api.views.register(VIEW_ID, {
      slot: 'editorArea',
      component: () => createElement(EditorView, { onRetry: handleRetry }),
      priority: 10,
    })

    api.events.on<FileOpenPayload>(EVENT_FILE_OPEN, (payload) => {
      if (!payload || typeof payload.relpath !== 'string') return
      void loadFile(payload)
    })

    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      lastPayload = null
      useEditorStore.getState().setFile(null)
      useEditorStore.getState().setError(null)
      useEditorStore.getState().setLoading(false)
    })
  },
}
