import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { viewRegistry } from '../../../workspace'
import { EditorView } from './EditorView'
import { markdownViewCreator } from './MarkdownView'
import { useEditorStore, isDirty } from './editorStore'
import { setEditorRuntime } from './runtime'
import { makeEditorClient } from './kernelClient'
import { makeSessionManager } from './sessionManager'

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
const COMMAND_NEW_UNTITLED = 'nexus.editor.newUntitled'
const COMMAND_CLOSE_ALL = 'nexus.editor.closeAll'
const CONTEXT_KEY_HAS_ACTIVE_TAB = 'nexus.editor.hasActiveTab'
const CONTEXT_KEY_ACTIVE_TAB_DIRTY = 'nexus.editor.activeTabDirty'

// Configuration keys read by the editor at runtime via
// api.configuration.getValue. The Settings panel (core.settings) auto-
// generates UI from the schema we register in `activate`.
const CONFIG_CONFIRM_CLOSE_DIRTY = 'nexus.editor.confirmCloseDirty'
const CONFIG_DEFAULT_MODE = 'nexus.editor.defaultMode'

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
        { id: COMMAND_NEW_UNTITLED, title: 'New Untitled Tab', category: 'Editor' },
        { id: COMMAND_CLOSE_ALL, title: 'Close All Tabs', category: 'Editor' },
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
    // Phase 3: shell now acquires a kernel session for every markdown
    // tab and hydrates content via `get_markdown` so what the user sees
    // round-trips through the same parser/serializer pair that `save`
    // writes back — no parallel `storage::read_file` for `.md` files.
    // Non-markdown files keep the storage-read path (no editor session
    // lifecycle for binaries / code files). See
    // `docs/editor-transaction-wiring-plan.md` §Phase 3.
    const editorClient = makeEditorClient(api.kernel)
    // Phase 4: pass the kernel API so the manager can open a
    // `com.nexus.editor.changed.<relpath>` subscription on acquire.
    const sessionManager = makeSessionManager(editorClient, api.kernel)

    /** `true` iff the editor should treat `name` (or relpath) as a
     *  markdown file eligible for a kernel session. Matches the
     *  extensions registered via `viewRegistry.registerExtensions`. */
    const isMarkdownPath = (name: string): boolean => {
      const lower = name.toLowerCase()
      return lower.endsWith('.md') || lower.endsWith('.markdown')
    }

    /** Hydrate a markdown tab via the editor plugin: acquire a session
     *  (which parses the on-disk file into a block tree) then pull the
     *  canonical serialized form. The cached snapshot is kept alive by
     *  the refcount until `release` is called — we pair this acquire
     *  with a release in the tab-removed subscription below. */
    const loadMarkdownContent = async (relpath: string): Promise<void> => {
      try {
        await sessionManager.acquire(relpath)
        const content = await editorClient.getMarkdown(relpath)
        useEditorStore.getState().setTabContent(relpath, content)
      } catch (err) {
        useEditorStore.getState().setTabError(relpath, String(err))
      }
    }

    /** Hydrate a non-markdown tab via the storage plugin — same path
     *  as the pre-Phase-3 implementation. Binaries / code files don't
     *  round-trip through the editor block tree. */
    const loadStorageContent = async (relpath: string): Promise<void> => {
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
    }

    const loadFile = async (payload: FileOpenPayload) => {
      const store = useEditorStore.getState()
      const isNew = store.openTab(payload.relpath, payload.name)
      // Already-open file: openTab raised it active; no refetch.
      if (!isNew) return

      // openTab seeds new tabs in 'preview' mode; honour the user's
      // default-mode preference if they've flipped it to 'source'.
      const defaultMode = api.configuration.getValue<string>(CONFIG_DEFAULT_MODE, 'preview')
      if (defaultMode === 'source') {
        useEditorStore.getState().setMode(payload.relpath, 'source')
      }

      if (isMarkdownPath(payload.name) || isMarkdownPath(payload.relpath)) {
        await loadMarkdownContent(payload.relpath)
      } else {
        await loadStorageContent(payload.relpath)
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
        if (isMarkdownPath(tab.name) || isMarkdownPath(relpath)) {
          await loadMarkdownContent(relpath)
        } else {
          await loadStorageContent(relpath)
        }
      })()
    }

    /**
     * Shared close-tab entry point used by both the ×-click handler
     * and the `nexus.editor.closeTab` command. If the tab is dirty,
     * shows the shared confirm modal (api.input.confirm) — cancelling
     * aborts. The async path means the close happens one tick later
     * than before, which is fine since both call sites are
     * fire-and-forget.
     */
    const confirmAndClose = async (relpath: string) => {
      const tab = useEditorStore.getState().tabs.find((t) => t.relpath === relpath)
      if (!tab) return
      if (isDirty(tab)) {
        // Power users can disable the confirm via Settings — the
        // default keeps the safety net on.
        const shouldConfirm = api.configuration.getValue(CONFIG_CONFIRM_CLOSE_DIRTY, true)
        if (shouldConfirm) {
          const ok = await api.input.confirm(`${tab.name} has unsaved changes. Close anyway?`)
          if (!ok) return
        }
      }
      useEditorStore.getState().closeTab(relpath)
    }

    // Phase 7: legacy SlotRegistry slot:'editorArea' entry removed.
    // `.md` opens now land as leaves of type 'markdown' in the main dock.
    viewRegistry.register(
      'markdown',
      markdownViewCreator(
        () => createElement(EditorView, { onRetry: handleRetry }),
        sessionManager,
      ),
    )
    viewRegistry.registerExtensions(['md', 'markdown'], 'markdown')

    // Settings panel auto-generates UI from this. Defaults match the
    // pre-settings behaviour so existing users don't see a regression.
    api.configuration.register({
      pluginId: 'nexus.editor',
      title: 'Editor',
      order: 10,
      schema: [
        {
          key: CONFIG_CONFIRM_CLOSE_DIRTY,
          title: 'Confirm before closing dirty tabs',
          description:
            'Show a confirmation dialog when closing a tab with unsaved changes. Disable for a faster keyboard-driven flow.',
          type: 'boolean',
          default: true,
        },
        {
          key: CONFIG_DEFAULT_MODE,
          title: 'Default mode for new tabs',
          description:
            'Whether newly-opened markdown files start in rendered preview or raw source. Read at tab-open time.',
          type: 'select',
          default: 'preview',
          options: ['preview', 'source'],
        },
      ],
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

    const openUntitled = () => {
      const existing = useEditorStore.getState().tabs
      let n = 1
      const names = new Set(existing.map((t) => t.relpath))
      while (names.has(`untitled-${n}`)) n++
      const relpath = `untitled-${n}`
      useEditorStore.getState().openUntitled(relpath, relpath)
    }

    const closeAll = async () => {
      // Snapshot relpaths up front — confirmAndClose mutates the tabs
      // array, iterating the live array would skip entries.
      const relpaths = useEditorStore.getState().tabs.map((t) => t.relpath)
      for (const relpath of relpaths) {
        await confirmAndClose(relpath)
      }
    }

    api.commands.register(COMMAND_NEW_UNTITLED, async () => {
      openUntitled()
    })

    api.commands.register(COMMAND_CLOSE_ALL, async () => {
      await closeAll()
    })

    setEditorRuntime({
      confirmAndClose,
      openUntitled,
      closeAll,
      kernelClient: editorClient,
      sessionManager,
      reportBridgeError: (message, err) => {
        api.notifications.show({
          type: 'error',
          message: `${message}: ${err instanceof Error ? err.message : String(err)}`,
        })
      },
    })

    /**
     * Write bytes via the storage plugin. Used by the non-markdown
     * save branch and by the untitled → named transition to seed the
     * file before a kernel session is opened on top of it.
     */
    const writeStorageFile = async (
      relpath: string,
      content: string,
    ): Promise<void> => {
      const bytes = Array.from(new TextEncoder().encode(content))
      await api.kernel.invoke<unknown>(STORAGE_PLUGIN_ID, WRITE_FILE_COMMAND, {
        path: relpath,
        bytes,
      })
    }

    api.commands.register(COMMAND_SAVE, async () => {
      const s = useEditorStore.getState()
      const tab = s.tabs.find((t) => t.relpath === s.activeRelpath)
      if (!tab) return
      if (!isDirty(tab)) return

      const isMd = isMarkdownPath(tab.name) || isMarkdownPath(tab.relpath)
      const hasSession = sessionManager.refcount(tab.relpath) > 0

      try {
        if (isMd && hasSession) {
          // Named markdown file with a live session — go through the
          // kernel so the bytes on disk match the in-memory block
          // tree byte-for-byte. `save` runs
          // `MarkdownSerializer::serialize` under the session lock and
          // hands off to `com.nexus.storage::write_file` atomically
          // (see `crates/nexus-editor/src/core_plugin.rs` ~L370).
          await editorClient.saveSession(tab.relpath)
          useEditorStore.getState().markSaved(tab.relpath)
          return
        }

        if (isMd && !hasSession) {
          // Untitled markdown (or a markdown tab that failed to
          // acquire earlier). We need an on-disk file before the
          // editor plugin can open a session for it, so:
          //   1. Serialize the current in-memory `content` via
          //      storage::write_file (creates / overwrites the file).
          //   2. Re-key the tab from the untitled placeholder to the
          //      real relpath (if they differ — for now the new
          //      relpath IS the old one, since the untitled-rename
          //      flow routes through a separate UI gesture; this
          //      branch mostly handles "file existed but session
          //      acquire failed" today). Still route through
          //      `renameTab` so the revision maps follow.
          //   3. Open a session and seed savedRevision so future
          //      saves take the kernel path above.
          const newRelpath = tab.relpath
          await writeStorageFile(newRelpath, tab.content)
          if (newRelpath !== tab.relpath) {
            useEditorStore.getState().renameTab(tab.relpath, newRelpath)
          }
          // Mark clean against current content before opening the
          // session — if `acquire` races a concurrent edit, the
          // transaction bridge will advance `sessionRevision` and
          // `isDirty` will flip back to true next paint.
          useEditorStore.getState().markSaved(newRelpath)
          try {
            await sessionManager.acquire(newRelpath)
            // acquire seeds sessionRevision + savedRevision from the
            // open-time snapshot, so the tab stays clean until the
            // next local edit.
          } catch (acquireErr) {
            // Acquire failure after a successful write is non-fatal —
            // subsequent saves will re-try the acquire via
            // loadMarkdownContent / retry. Surface but don't throw.
            api.notifications.show({
              type: 'warning',
              message: `Save wrote the file, but could not reopen an editor session: ${
                acquireErr instanceof Error ? acquireErr.message : String(acquireErr)
              }`,
            })
          }
          return
        }

        // Non-markdown named file — same storage-write as pre-Phase-6.
        await writeStorageFile(tab.relpath, tab.content)
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

      // Phase 3 refcount pairing: every `loadFile` that acquired a
      // markdown session needs a matching release when the tab goes
      // away. Detect tabs that existed in `prev` but are gone from
      // `state` and release them. The refcount lets the leaf-held
      // acquire (MarkdownView.onOpen) keep the session alive if the
      // leaf is still mounted — e.g. during a re-layout.
      const currentPaths = new Set(state.tabs.map((t) => t.relpath))
      for (const prevTab of prev.tabs) {
        if (currentPaths.has(prevTab.relpath)) continue
        if (isMarkdownPath(prevTab.name) || isMarkdownPath(prevTab.relpath)) {
          void sessionManager.release(prevTab.relpath)
        }
      }
    })
  },
}
