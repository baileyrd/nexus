import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { EditorView } from './EditorView'
import { useEditorStore, isDirty } from './editorStore'

const VIEW_ID = 'nexus.editor.view'
const EVENT_FILE_OPEN = 'files:open'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
// Verified against crates/nexus-storage/src/core_plugin.rs::dispatch:
//   HANDLER_READ_FILE  args `{ "path": String }` → `{ "bytes": Vec<u8> }`.
//   HANDLER_WRITE_FILE args `{ "path": String, "bytes": Vec<u8> }`
//                      → `FileMetadata` (ignored here).
// Arg key is `path`, NOT `relpath` (unlike `list_dir` / `create_file`).
// serde_json encodes `Vec<u8>` as a JSON number array, so we pass the
// UTF-8 bytes through `Array.from(new TextEncoder().encode(...))`.
const READ_FILE_COMMAND = 'read_file'
const WRITE_FILE_COMMAND = 'write_file'

const COMMAND_CLOSE_TAB = 'nexus.editor.closeTab'
const COMMAND_SAVE = 'nexus.editor.save'
const CONTEXT_KEY_HAS_ACTIVE_TAB = 'nexus.editor.hasActiveTab'
const CONTEXT_KEY_ACTIVE_TAB_DIRTY = 'nexus.editor.activeTabDirty'

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
        { id: COMMAND_SAVE, title: 'Save', category: 'Editor' },
      ],
      keybindings: [
        {
          command: COMMAND_CLOSE_TAB,
          key: 'ctrl+w',
          mac: 'cmd+w',
          when: CONTEXT_KEY_HAS_ACTIVE_TAB,
        },
        {
          command: COMMAND_SAVE,
          key: 'ctrl+s',
          mac: 'cmd+s',
          when: CONTEXT_KEY_ACTIVE_TAB_DIRTY,
        },
      ],
      contextKeys: [
        {
          key: CONTEXT_KEY_HAS_ACTIVE_TAB,
          description: 'True when the editor has at least one open tab.',
          type: 'boolean',
        },
        {
          key: CONTEXT_KEY_ACTIVE_TAB_DIRTY,
          description: 'True when the active tab has unsaved changes.',
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

    /**
     * Shared close-tab entry point used by both the ×-click handler
     * and the `nexus.editor.closeTab` command. If the tab is dirty,
     * shows a browser confirm() — cancelling aborts. `window.confirm`
     * keeps this lean; a bespoke modal is a follow-up (see plan).
     */
    const confirmAndClose = (relpath: string) => {
      const tab = useEditorStore.getState().tabs.find((t) => t.relpath === relpath)
      if (!tab) return
      if (isDirty(tab)) {
        const ok = window.confirm(`${tab.name} has unsaved changes. Close anyway?`)
        if (!ok) return
      }
      useEditorStore.getState().closeTab(relpath)
    }

    api.views.register(VIEW_ID, {
      slot: 'editorArea',
      component: () =>
        createElement(EditorView, {
          onRetry: handleRetry,
          onRequestClose: confirmAndClose,
        }),
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
      if (s.activeRelpath) confirmAndClose(s.activeRelpath)
    })

    api.commands.register(COMMAND_SAVE, async () => {
      const s = useEditorStore.getState()
      const tab = s.tabs.find((t) => t.relpath === s.activeRelpath)
      if (!tab) return
      if (!isDirty(tab)) return
      try {
        // serde_json decodes Vec<u8> from a JSON number array — pass
        // the UTF-8 bytes that way. An alternative base64 envelope
        // would require matching on the Rust side; we use the
        // straightforward number-array path that `write_file`
        // already expects.
        const bytes = Array.from(new TextEncoder().encode(tab.content))
        await api.kernel.invoke<unknown>(
          STORAGE_PLUGIN_ID,
          WRITE_FILE_COMMAND,
          { path: tab.relpath, bytes },
        )
        useEditorStore.getState().markSaved(tab.relpath)
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Save failed: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })

    // Seed + sync context keys to the store. We track two
    // transitions: (a) activeRelpath presence, (b) whether the
    // active tab is dirty. `subscribe` is called on every store
    // mutation but we only re-publish on an actual transition to
    // avoid spurious context churn.
    const seedHasActive = useEditorStore.getState().activeRelpath !== null
    const seedActiveTab = useEditorStore
      .getState()
      .tabs.find((t) => t.relpath === useEditorStore.getState().activeRelpath)
    const seedDirty = !!seedActiveTab && isDirty(seedActiveTab)
    api.context.set(CONTEXT_KEY_HAS_ACTIVE_TAB, seedHasActive)
    api.context.set(CONTEXT_KEY_ACTIVE_TAB_DIRTY, seedDirty)

    useEditorStore.subscribe((state, prev) => {
      const hasActive = state.activeRelpath !== null
      const prevHasActive = prev.activeRelpath !== null
      if (hasActive !== prevHasActive) {
        api.context.set(CONTEXT_KEY_HAS_ACTIVE_TAB, hasActive)
      }
      const activeTab = state.tabs.find((t) => t.relpath === state.activeRelpath)
      const prevActiveTab = prev.tabs.find((t) => t.relpath === prev.activeRelpath)
      const dirty = !!activeTab && isDirty(activeTab)
      const prevDirty = !!prevActiveTab && isDirty(prevActiveTab)
      if (dirty !== prevDirty) {
        api.context.set(CONTEXT_KEY_ACTIVE_TAB_DIRTY, dirty)
      }
    })
  },
}
