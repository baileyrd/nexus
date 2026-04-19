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

const COMMAND_CLOSE_TAB = 'nexus.editor.closeTab'
const CONTEXT_KEY_HAS_ACTIVE_TAB = 'nexus.editor.hasActiveTab'

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
    contributes: {
      commands: [
        { id: COMMAND_CLOSE_TAB, title: 'Close Tab', category: 'Editor' },
      ],
      keybindings: [
        {
          command: COMMAND_CLOSE_TAB,
          key: 'ctrl+w',
          mac: 'cmd+w',
          when: CONTEXT_KEY_HAS_ACTIVE_TAB,
        },
      ],
      contextKeys: [
        {
          key: CONTEXT_KEY_HAS_ACTIVE_TAB,
          description: 'True when the editor has at least one open tab.',
          type: 'boolean',
        },
      ],
    },
  },

  async activate(api: PluginAPI) {
    const loadFile = async (payload: FileOpenPayload) => {
      const store = useEditorStore.getState()
      const isNew = store.openTab(payload.relpath, payload.name)
      // Already-open file: openTab raised it active; no refetch.
      if (!isNew) return

      try {
        const resp = await api.kernel.invoke<ReadFileResponse>(
          STORAGE_PLUGIN_ID,
          READ_FILE_COMMAND,
          { path: payload.relpath },
        )
        const content = decodeUtf8(resp.bytes ?? [])
        useEditorStore.getState().setTabContent(payload.relpath, content)
      } catch (err) {
        useEditorStore.getState().setTabError(payload.relpath, String(err))
      }
    }

    const handleRetry = (relpath: string) => {
      const tab = useEditorStore.getState().tabs.find((t) => t.relpath === relpath)
      if (!tab) return
      // Reset to a loading state and re-invoke. We bypass `openTab`
      // here because the tab already exists — just flip it back to
      // loading and re-read from the kernel.
      useEditorStore.setState((s) => ({
        tabs: s.tabs.map((t) =>
          t.relpath === relpath ? { ...t, loading: true, error: null } : t,
        ),
      }))
      void (async () => {
        try {
          const resp = await api.kernel.invoke<ReadFileResponse>(
            STORAGE_PLUGIN_ID,
            READ_FILE_COMMAND,
            { path: relpath },
          )
          const content = decodeUtf8(resp.bytes ?? [])
          useEditorStore.getState().setTabContent(relpath, content)
        } catch (err) {
          useEditorStore.getState().setTabError(relpath, String(err))
        }
      })()
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
      useEditorStore.getState().clear()
    })

    api.commands.register(COMMAND_CLOSE_TAB, async () => {
      const s = useEditorStore.getState()
      if (s.activeRelpath) s.closeTab(s.activeRelpath)
    })

    // Seed + sync the context key to the store's `activeRelpath`.
    // Fire-and-forget reactive side effect — `subscribe` gets the
    // next state and the previous state so we only re-set the
    // context key on an actual transition.
    api.context.set(
      CONTEXT_KEY_HAS_ACTIVE_TAB,
      useEditorStore.getState().activeRelpath !== null,
    )
    useEditorStore.subscribe((state, prev) => {
      const has = state.activeRelpath !== null
      const hadBefore = prev.activeRelpath !== null
      if (has !== hadBefore) {
        api.context.set(CONTEXT_KEY_HAS_ACTIVE_TAB, has)
      }
    })
  },
}
