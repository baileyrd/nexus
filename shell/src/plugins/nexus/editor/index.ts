import { createElement } from 'react'
import { open as openInShell } from '@tauri-apps/plugin-shell'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { viewRegistry, workspace } from '../../../workspace'
import type { Leaf, Tabs, WorkspaceParent } from '../../../workspace'
import { EditorView } from './EditorView'
import { markdownViewCreator } from './MarkdownView'
import { emptyViewCreator, EMPTY_VIEW_TYPE } from './EmptyView'
import { useEditorStore, isDirty, type EditorTabMode } from './editorStore'
import { openSearchPanel } from '@codemirror/search'
import { setEditorRuntime, getActiveCmView } from './runtime'
import { makeEditorClient } from './kernelClient'
import { makeSessionManager } from './sessionManager'
import { installSlashMenuStyles } from './cm/slashCommand'
import { installBlockHandleStyles } from './cm/blockHandle'
import { installInlineToolbarStyles } from './cm/inlineToolbar'
import { useWorkspaceStore } from '../workspace/workspaceStore'
import { useFilesStore } from '../files/filesStore'

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
const COMMAND_TOGGLE_MODE = 'nexus.editor.toggleMode'
const COMMAND_FIND = 'nexus.editor.find'
const COMMAND_REPLACE = 'nexus.editor.replace'
const COMMAND_COPY_REL_PATH = 'nexus.editor.copyRelativePath'
const COMMAND_COPY_ABS_PATH = 'nexus.editor.copyAbsolutePath'
const COMMAND_REVEAL_IN_NAV = 'nexus.editor.revealInNavigation'
const COMMAND_REVEAL_IN_OS = 'nexus.editor.revealInOS'
const COMMAND_OPEN_DEFAULT_APP = 'nexus.editor.openInDefaultApp'
const COMMAND_DELETE_FILE = 'nexus.editor.deleteFile'
const DELETE_FILE_HANDLER = 'delete_file'
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
        { id: COMMAND_TOGGLE_MODE, title: 'Toggle Reading View', category: 'Editor' },
        { id: COMMAND_FIND, title: 'Find', category: 'Editor' },
        { id: COMMAND_REPLACE, title: 'Replace', category: 'Editor' },
        { id: COMMAND_COPY_REL_PATH, title: 'Copy Path (relative)', category: 'Editor' },
        { id: COMMAND_COPY_ABS_PATH, title: 'Copy Path (absolute)', category: 'Editor' },
        { id: COMMAND_REVEAL_IN_NAV, title: 'Reveal File in Navigation', category: 'Editor' },
        { id: COMMAND_REVEAL_IN_OS, title: 'Show in System Explorer', category: 'Editor' },
        { id: COMMAND_OPEN_DEFAULT_APP, title: 'Open in Default App', category: 'Editor' },
        { id: COMMAND_DELETE_FILE, title: 'Delete File', category: 'Editor' },
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
    installSlashMenuStyles()
    installBlockHandleStyles()
    installInlineToolbarStyles()
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

    /** Enumerate every leaf inside the main dock (rootSplit). Side
     *  docks and floating windows are excluded — file-opens should
     *  never mount into the sidebar or a popout. */
    const collectMainLeaves = (node: WorkspaceParent, acc: Leaf[]): void => {
      if (node.kind === 'tabs') {
        for (const l of node.leaves) acc.push(l)
        return
      }
      if (node.kind === 'split') {
        for (const c of node.children) collectMainLeaves(c, acc)
        return
      }
      const withChild = node as { child?: WorkspaceParent }
      if (withChild.child) collectMainLeaves(withChild.child, acc)
    }

    /** First Tabs node found walking rootSplit depth-first — target
     *  for appending a new tab when no empty leaf is available. */
    const findFirstMainTabs = (node: WorkspaceParent): Tabs | null => {
      if (node.kind === 'tabs') return node
      if (node.kind === 'split') {
        for (const c of node.children) {
          const t = findFirstMainTabs(c)
          if (t) return t
        }
        return null
      }
      const withChild = node as { child?: WorkspaceParent }
      if (withChild.child) return findFirstMainTabs(withChild.child)
      return null
    }

    /** Derive a viewType from a filename. Falls back to `markdown` if
     *  the extension isn't registered — that matches what the default
     *  layout expects the main dock to render for content files. */
    const viewTypeForFile = (name: string, relpath: string): string => {
      const candidates = [name, relpath]
      for (const s of candidates) {
        const dot = s.lastIndexOf('.')
        if (dot < 0) continue
        const ext = s.slice(dot + 1).toLowerCase()
        const type = viewRegistry.getTypeForExt(ext)
        if (type) return type
      }
      return 'markdown'
    }

    /** Ensure a main-pane leaf is rendering `payload.relpath` with the
     *  correct view type, and raise it active. Strategy:
     *    1. If a main leaf already holds this relpath → reveal it.
     *    2. Else prefer the currently-active main leaf if it is an
     *       `empty` placeholder (first-open flow from defaultLayout).
     *    3. Else reuse any `empty` main leaf.
     *    4. Else append a new tab to the first main Tabs group.
     *  The final setViewState passes `active: true` so the workspace
     *  store's active-leaf-change bridge fires, keeping sidebars and
     *  status in sync with what the main pane shows. */
    const mountFileInMainLeaf = (payload: FileOpenPayload): void => {
      const type = viewTypeForFile(payload.name, payload.relpath)

      const mainLeaves: Leaf[] = []
      collectMainLeaves(workspace.rootSplit, mainLeaves)

      const existing = mainLeaves.find((l) => {
        if (l.view?.viewType !== type) return false
        const st = l.view.getState() as { relpath?: unknown } | undefined
        return typeof st?.relpath === 'string' && st.relpath === payload.relpath
      })
      if (existing) {
        workspace.revealLeaf(existing)
        return
      }

      const activeId = workspace.activeLeafId
      const activeLeaf = activeId ? workspace.leaves.get(activeId) ?? null : null
      let target: Leaf | null = null
      if (
        activeLeaf &&
        mainLeaves.includes(activeLeaf) &&
        activeLeaf.view?.viewType === 'empty'
      ) {
        target = activeLeaf
      }
      if (!target) {
        target = mainLeaves.find((l) => l.view?.viewType === 'empty') ?? null
      }

      if (!target) {
        const tabs = findFirstMainTabs(workspace.rootSplit)
        if (!tabs) return
        target = workspace.createLeaf(tabs)
        tabs.leaves.push(target)
        tabs.activeIndex = tabs.leaves.length - 1
        workspace.emit('layout-change')
      }

      void target.setViewState({
        type,
        state: { relpath: payload.relpath },
        active: true,
      })
      workspace.revealLeaf(target)
    }

    const loadFile = async (payload: FileOpenPayload) => {
      const store = useEditorStore.getState()
      const isNew = store.openTab(payload.relpath, payload.name)

      // Mount / reveal the main-pane leaf for this file. Without this
      // step the editor store holds the tab but the main dock still
      // renders whatever view type the target leaf was on (typically
      // `empty` from the default layout), so the user sees a blank
      // pane. Done regardless of `isNew` so re-opening a file also
      // raises its existing leaf into view.
      mountFileInMainLeaf(payload)

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
        (relpath) => createElement(EditorView, { relpath, onRetry: handleRetry }),
        sessionManager,
      ),
    )
    viewRegistry.registerExtensions(['md', 'markdown'], 'markdown')

    // Override the default no-op empty view (shell/src/workspace/ViewRegistry.ts)
    // with one that renders the Obsidian-style action links — used by
    // the tab-strip `+` button and any other leaf that lands on the
    // empty type (e.g. restored placeholder leaves).
    viewRegistry.register(EMPTY_VIEW_TYPE, emptyViewCreator)

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

    const activeTabRelpath = (): string | null => {
      const s = useEditorStore.getState()
      return s.activeRelpath ?? null
    }

    const joinAbsPath = (root: string, rel: string): string => {
      const sep = root.includes('\\') && !root.includes('/') ? '\\' : '/'
      const trimmed = root.endsWith('/') || root.endsWith('\\') ? root.slice(0, -1) : root
      return `${trimmed}${sep}${rel}`
    }

    const parentDir = (path: string): string => {
      const idx = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
      if (idx <= 0) return path
      return path.slice(0, idx)
    }

    api.commands.register(COMMAND_TOGGLE_MODE, async () => {
      const s = useEditorStore.getState()
      const tab = s.tabs.find((t) => t.relpath === s.activeRelpath)
      if (!tab) return
      const next: EditorTabMode = tab.mode === 'preview' ? 'source' : 'preview'
      s.setMode(tab.relpath, next)
    })

    api.commands.register(COMMAND_FIND, async () => {
      const view = getActiveCmView()
      if (!view) {
        api.notifications.show({
          type: 'info',
          message: 'Find requires an active editor in source mode.',
        })
        return
      }
      view.focus()
      openSearchPanel(view)
    })

    api.commands.register(COMMAND_REPLACE, async () => {
      const view = getActiveCmView()
      if (!view) {
        api.notifications.show({
          type: 'info',
          message: 'Replace requires an active editor in source mode.',
        })
        return
      }
      view.focus()
      openSearchPanel(view)
    })

    api.commands.register(COMMAND_COPY_REL_PATH, async () => {
      const relpath = activeTabRelpath()
      if (!relpath) return
      try {
        await navigator.clipboard.writeText(relpath)
        api.notifications.show({ type: 'info', message: 'Copied relative path.' })
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Copy failed: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })

    api.commands.register(COMMAND_COPY_ABS_PATH, async () => {
      const relpath = activeTabRelpath()
      if (!relpath) return
      const root = useWorkspaceStore.getState().rootPath
      if (!root) {
        api.notifications.show({
          type: 'warning',
          message: 'No workspace open — absolute path is unavailable.',
        })
        return
      }
      try {
        await navigator.clipboard.writeText(joinAbsPath(root, relpath))
        api.notifications.show({ type: 'info', message: 'Copied absolute path.' })
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Copy failed: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })

    api.commands.register(COMMAND_REVEAL_IN_NAV, async () => {
      const relpath = activeTabRelpath()
      if (!relpath) return
      const filesStore = useFilesStore.getState()
      const segments = relpath.split('/').filter((s) => s.length > 0)
      let acc = ''
      for (let i = 0; i < segments.length - 1; i++) {
        acc = acc ? `${acc}/${segments[i]}` : segments[i]
        filesStore.setExpanded(acc, true)
      }
      filesStore.setSelected(relpath)
      const leaf = await workspace.ensureLeafOfType('file-explorer', 'left')
      workspace.revealLeaf(leaf)
    })

    api.commands.register(COMMAND_OPEN_DEFAULT_APP, async () => {
      const relpath = activeTabRelpath()
      if (!relpath) return
      const root = useWorkspaceStore.getState().rootPath
      if (!root) {
        api.notifications.show({
          type: 'warning',
          message: 'No workspace open.',
        })
        return
      }
      try {
        await openInShell(joinAbsPath(root, relpath))
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Open failed: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })

    api.commands.register(COMMAND_REVEAL_IN_OS, async () => {
      const relpath = activeTabRelpath()
      if (!relpath) return
      const root = useWorkspaceStore.getState().rootPath
      if (!root) {
        api.notifications.show({
          type: 'warning',
          message: 'No workspace open.',
        })
        return
      }
      try {
        await openInShell(parentDir(joinAbsPath(root, relpath)))
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Reveal failed: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })

    api.commands.register(COMMAND_DELETE_FILE, async () => {
      const relpath = activeTabRelpath()
      if (!relpath) return
      if (/^untitled-\d+$/i.test(relpath)) {
        await confirmAndClose(relpath)
        return
      }
      const ok = await api.input.confirm(
        `Delete "${relpath}"? This cannot be undone.`,
      )
      if (!ok) return
      try {
        await api.kernel.invoke<unknown>(STORAGE_PLUGIN_ID, DELETE_FILE_HANDLER, {
          path: relpath,
        })
        useEditorStore.getState().closeTab(relpath)
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Delete failed: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
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

    // Keep `editorStore.activeRelpath` in sync with the focused
    // workspace leaf so COMMAND_CLOSE_TAB / COMMAND_SAVE and the
    // editor context keys track whatever file the user is currently
    // editing. Each markdown leaf owns exactly one file via its
    // view state (see `MarkdownView.state.relpath`); non-markdown
    // leaves clear the active relpath so command predicates don't
    // pretend there's still an editor focused.
    //
    // When a markdown leaf becomes active whose relpath has no tab
    // in the editor store yet, lazily seed it via `loadFile`. This
    // covers two flows that don't go through `files:open`:
    //   (a) workspace hydration from a persisted layout — MarkdownView
    //       leaves are recreated via `setViewState` but no file-open
    //       event fires, so the editor store stays empty until the
    //       user re-opens the file from the sidebar;
    //   (b) tab-click onto a leaf that was created by the mount flow
    //       but whose editor tab was evicted (e.g. closeAll) while
    //       the leaf lived on.
    // Without this, the active leaf renders the editor's "Select a
    // file to view" empty state even though the tab strip shows the
    // file as active.
    workspace.on('active-leaf-change', (payload) => {
      const leaf = (payload as {
        leaf?: {
          view: { viewType: string } | null
          getViewState: () => { state?: unknown }
        }
      } | undefined)?.leaf
      // leaf: null means the last leaf in the main dock was detached
      // (closed) or the workspace has no active leaf. Either way the
      // editor has nothing to focus.
      if (!leaf) {
        useEditorStore.getState().setActive(null)
        return
      }
      // Sidecar leaves (outline, backlinks, terminal, search, etc.)
      // becoming active must NOT clear editorStore.activeRelpath —
      // dependents like the outline plugin recompute off that value,
      // and clearing it every time the user clicks a right-dock tab
      // would wipe their panels. Only react to markdown leaves here;
      // non-markdown editor-dock leaves (the `empty` new-tab placeholder)
      // still clear below since they ARE in the main editor area.
      if (leaf.view?.viewType !== 'markdown') {
        if (leaf.view?.viewType === 'empty') {
          useEditorStore.getState().setActive(null)
        }
        return
      }
      const vs = leaf.getViewState()
      const relpath =
        vs.state && typeof vs.state === 'object' && 'relpath' in vs.state
          ? (vs.state as Record<string, unknown>).relpath
          : undefined
      if (typeof relpath !== 'string') return

      const hasTab = useEditorStore
        .getState()
        .tabs.some((t) => t.relpath === relpath)
      if (!hasTab) {
        // Derive the display name the same way MarkdownView.getDisplayText
        // does — basename of the relpath. `loadFile` will hydrate
        // content via the kernel (markdown) or storage (other) path.
        const sepIdx = Math.max(relpath.lastIndexOf('/'), relpath.lastIndexOf('\\'))
        const name = sepIdx >= 0 ? relpath.slice(sepIdx + 1) : relpath
        void loadFile({ relpath, name })
        return
      }
      useEditorStore.getState().setActive(relpath)
    })

    // Hydration-path seed. `workspaceStore.hydrate` drives each persisted
    // leaf through `setViewState(sLeaf.viewState)`, but `getViewState()`
    // intentionally omits the `active` flag (it is a workspace-level
    // property recorded against `json.active`). That means `Leaf.ts`
    // never emits `active-leaf-change` during hydrate — only
    // `view-changed` — so the `active-leaf-change` handler above doesn't
    // fire for the restored active leaf. Without this seed the user sees
    // the "Select a file to view" empty state on app boot even though
    // the tab strip shows a.md as active.
    //
    // On `layout-ready` (fired once all hydrate setViewState calls
    // resolve), walk every markdown leaf and `loadFile` for any relpath
    // we don't already have a tab for, then set the active tab to the
    // currently-active leaf's relpath if it is a markdown leaf.
    const seedFromLayout = () => {
      const leaves = workspace.getLeavesOfType('markdown')
      for (const leaf of leaves) {
        const st = leaf.view?.getState() as { relpath?: unknown } | undefined
        const relpath = typeof st?.relpath === 'string' ? st.relpath : null
        if (!relpath) continue
        const hasTab = useEditorStore.getState().tabs.some((t) => t.relpath === relpath)
        if (hasTab) continue
        const sepIdx = Math.max(relpath.lastIndexOf('/'), relpath.lastIndexOf('\\'))
        const name = sepIdx >= 0 ? relpath.slice(sepIdx + 1) : relpath
        void loadFile({ relpath, name })
      }
    }
    workspace.on('layout-ready', seedFromLayout)
    // If the workspace has already hydrated before this plugin activated
    // (activation order is not guaranteed), seed immediately — any leaves
    // already present get their tabs hydrated.
    queueMicrotask(seedFromLayout)

    // Reconcile editor tabs against markdown leaves on layout changes.
    // When the user closes a tab via the workspace-level × (which calls
    // `workspace.detachLeaf`), only the workspace leaf is removed — the
    // editor store tab persists. Walk the markdown leaves and drop any
    // editor tab whose relpath no longer has a corresponding leaf.
    // Untitled tabs (no on-disk backing) are kept as-is: they are
    // owned by the editor store, not the workspace.
    const reconcileTabs = () => {
      const mdLeaves = workspace.getLeavesOfType('markdown')
      const mdRelpaths = new Set<string>()
      for (const leaf of mdLeaves) {
        const st = leaf.view?.getState() as { relpath?: unknown } | undefined
        if (typeof st?.relpath === 'string') mdRelpaths.add(st.relpath)
      }
      const store = useEditorStore.getState()
      for (const tab of store.tabs) {
        // Preserve untitled tabs — they have no backing workspace leaf
        // by design.
        if (/^untitled-\d+$/i.test(tab.relpath)) continue
        if (!mdRelpaths.has(tab.relpath)) {
          store.closeTab(tab.relpath)
        }
      }
    }
    workspace.on('layout-change', reconcileTabs)

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
