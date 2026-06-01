import { createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { DiffView } from './DiffView'
import './diffView.css'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { clientLogger } from '../../../clientLogger'
import { viewRegistry, workspace } from '../../../workspace'
import type { Leaf, Tabs, WorkspaceParent } from '../../../workspace'
import { EditorView } from './EditorView'
import { startMultibufferSync } from './multibuffer/sync'
import { markdownViewCreator } from './MarkdownView'
import { emptyViewCreator, EMPTY_VIEW_TYPE } from './EmptyView'
import { useEditorStore, isDirty, type EditorTabMode } from './editorStore'
// R10 / #193 — register this plugin's editor surface with the host seam so
// `api.editor` delegates to it instead of the host statically importing the
// editor store + fenced-code registry.
import { registerEditorHostSurface } from '../../../host/EditorHostSurface'
import { computeActiveEditor, activeEditorEquals } from '../../../host/activeEditor'
import { fencedCodeRegistry } from './cm/fencedCodeRegistry'
import { useEditorBlameStore } from './blameStore'
import { openSearchPanel } from '@codemirror/search'
import { setEditorRuntime, getActiveCmView } from './runtime'
import { revealBlockInView } from './cm/blockLinkNav'
import { revealLineInView } from './cm/revealLine'
import { makeEditorClient } from './kernelClient'
import { makeSessionManager } from './sessionManager'
import { DEFAULT_CODE_EXTENSIONS } from './codeMode'
import { installSlashMenuStyles } from './cm/slashCommand'
import {
  installBlockHandleStyles,
  setBlockRefDragBridge,
  setCommentBridge,
} from './cm/blockHandle'
import { createBlockRefDragBridge } from './blockRefDragBridge'
import { installInlineToolbarStyles } from './cm/inlineToolbar'
import { installMarginSuggestStyles } from './cm/marginSuggestions'
import { runSaveFormatHook } from './cm/saveFormatHooks'
import { LspIpc } from './cm/lspIpc'
// BL-141 Phase 3 — LSP → multibuffer converters.
import {
  locationsToExcerptRequests,
  workspaceEditToExcerptRequests,
  type LspLocation,
} from './cm/lspToExcerpts'
import {
  applyWorkspaceEdit,
  type LspWorkspaceEdit,
} from './cm/workspaceEdit'
import { createCommentsApi } from '../comments/commentsApi'
import { useWorkspaceStore } from '../workspace/workspaceStore'
import { useFilesStore } from '../files/filesStore'

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
const COMMAND_TOGGLE_READING_VIEW = 'nexus.editor.toggleReadingView'
const COMMAND_FIND = 'nexus.editor.find'
const COMMAND_REPLACE = 'nexus.editor.replace'
const COMMAND_COPY_REL_PATH = 'nexus.editor.copyRelativePath'
const COMMAND_COPY_ABS_PATH = 'nexus.editor.copyAbsolutePath'
const COMMAND_REVEAL_IN_NAV = 'nexus.editor.revealInNavigation'
const COMMAND_REVEAL_IN_OS = 'nexus.editor.revealInOS'
const COMMAND_OPEN_DEFAULT_APP = 'nexus.editor.openInDefaultApp'
const COMMAND_DELETE_FILE = 'nexus.editor.deleteFile'
// BL-079 — toggle the inline-blame annotations on the active tab.
// Reads / writes `useEditorBlameStore`; the editor view subscribes
// and remounts CM6 with / without the blame extension on flip.
const COMMAND_TOGGLE_BLAME = 'nexus.editor.toggleBlame'
// BL-079 — open the modal diff viewer for the active tab. Reads
// `com.nexus.git::diff_file` and renders the hunks unified.
const COMMAND_OPEN_DIFF = 'nexus.editor.openDiff'
// BL-141 Phase 3 — LSP find-references → multibuffer + LSP rename
// preview → multibuffer. Both surface LSP results in the new
// excerpt view shipped in BL-141 Phase 1/2 so the user can
// browse / preview before deciding to apply.
const COMMAND_LSP_FIND_REFERENCES = 'nexus.editor.lsp.findReferences'
const COMMAND_LSP_RENAME_PREVIEW = 'nexus.editor.lsp.renamePreview'

// BL-077 follow-up — LSP rename. Cursor-position rename across every
// file the symbol appears in; multi-file `WorkspaceEdit` applies via
// `applyWorkspaceEdit` (active tab through the live CM view, every
// other file through `com.nexus.storage::write_file`).
const COMMAND_LSP_RENAME = 'nexus.editor.lsp.rename'
// BL-077 follow-up — LSP code actions. Cursor-position code-action
// menu surfaced via `api.input.pick`; a chosen action's
// `WorkspaceEdit` applies through the same `applyWorkspaceEdit`
// path as rename. Command-only actions (no `edit`) require the
// LSP `workspace/executeCommand` surface that the host doesn't
// expose yet — those are listed in the picker for transparency
// but with a disabled message.
const COMMAND_LSP_CODE_ACTIONS = 'nexus.editor.lsp.codeActions'

// Tab-actions menu placeholders. Each one shows a "Coming soon"
// notification so users get feedback instead of a dead disabled row.
// Listed here so the manifest contributions and runtime registrations
// stay in sync.
const STUB_COMMANDS: ReadonlyArray<{ id: string; title: string; label: string }> = [
  { id: 'nexus.editor.stub.splitRight', title: 'Split right', label: 'Split right' },
  { id: 'nexus.editor.stub.splitDown', title: 'Split down', label: 'Split down' },
  {
    id: 'nexus.editor.stub.openInNewWindow',
    title: 'Open in new window',
    label: 'Open in new window',
  },
  {
    id: 'nexus.editor.stub.openLinkedView',
    title: 'Open linked view',
    label: 'Open linked view',
  },
  { id: 'nexus.editor.stub.rename', title: 'Rename file', label: 'Rename' },
  { id: 'nexus.editor.stub.moveTo', title: 'Move file to…', label: 'Move file to' },
  { id: 'nexus.editor.stub.bookmark', title: 'Bookmark file', label: 'Bookmark' },
  {
    id: 'nexus.editor.stub.addProperty',
    title: 'Add file property',
    label: 'Add file property',
  },
  {
    id: 'nexus.editor.stub.backlinksInDocument',
    title: 'Backlinks in document',
    label: 'Backlinks in document',
  },
  {
    id: 'nexus.editor.stub.versionHistory',
    title: 'Open version history',
    label: 'Version history',
  },
  {
    id: 'nexus.editor.stub.mergeFile',
    title: 'Merge entire file with…',
    label: 'Merge entire file',
  },
  { id: 'nexus.editor.stub.exportPdf', title: 'Export to PDF…', label: 'Export to PDF' },
]
const DELETE_FILE_HANDLER = 'delete_file'
const CONTEXT_KEY_HAS_ACTIVE_TAB = 'nexus.editor.hasActiveTab'
const CONTEXT_KEY_ACTIVE_TAB_DIRTY = 'nexus.editor.activeTabDirty'

// Configuration keys read by the editor at runtime via
// api.configuration.getValue. The Settings panel (core.settings) auto-
// generates UI from the schema we register in `activate`.
const CONFIG_CONFIRM_CLOSE_DIRTY = 'nexus.editor.confirmCloseDirty'
const CONFIG_DEFAULT_MODE = 'nexus.editor.defaultMode'
// BL-070: opt-in modal keybinding layer for the markdown editor.
const CONFIG_KEYBINDINGS = 'nexus.editor.keybindings'
// BL-075: comma-separated list of file extensions that open in code
// mode (raw CM6 with a language extension) rather than document mode
// (markdown block tree). Read at file-open time.
const CONFIG_CODE_FILE_EXTENSIONS = 'nexus.editor.codeFileExtensions'
// BL-139 — per-keystroke FIM edit prediction. Off by default per the
// BL DoD; the provider/model fields are informational (the actual
// route is decided by `com.nexus.ai`'s configured provider).
const CONFIG_EDIT_PREDICTION_ENABLED = 'nexus.editor.editPrediction.enabled'
const CONFIG_EDIT_PREDICTION_DEBOUNCE_MS = 'nexus.editor.editPrediction.debounceMs'
const CONFIG_EDIT_PREDICTION_PROVIDER = 'nexus.editor.editPrediction.provider'
const CONFIG_EDIT_PREDICTION_MODEL = 'nexus.editor.editPrediction.model'
// BL-142 Phase 2a — re-export the config key + default from
// `replKernels.ts` so the schema registration below stays in sync
// with the parser/resolver helpers.
import {
  CONFIG_REPL_KERNELS,
  REPL_KERNELS_DEFAULT_JSON,
} from './replKernels.ts'
// BL-142 Phase 2b.1 — tab-close teardown for REPL sessions.
import { makeReplClient } from './replClient.ts'
import { useReplStore } from './replStore.ts'
// BL-142 Phase 2b.2 — bus pump that routes
// `com.nexus.terminal.output.<sessionId>` events into the per-cell
// output buffer.
import { startReplOutputPump } from './replOutputPump.ts'
// BL-142 Phase 3 — Settings → REPL Kernels tab.
import { ReplKernelsTab } from './ReplKernelsTab'
// Visual settings — applied live via CSS custom properties on :root
// (see applyEditorCssVars below) and via prop flow to CodeMirrorHost.
const CONFIG_FONT_SIZE = 'nexus.editor.fontSize'
const CONFIG_LINE_HEIGHT = 'nexus.editor.lineHeight'
const CONFIG_LINE_NUMBERS = 'nexus.editor.lineNumbers'

function applyEditorCssVars(api: PluginAPI): void {
  const root = document.documentElement
  const apply = () => {
    const fontSize = api.configuration.getValue<number>(CONFIG_FONT_SIZE, 13)
    const lineHeight = api.configuration.getValue<number>(CONFIG_LINE_HEIGHT, 1.6)
    root.style.setProperty('--editor-font-size', `${fontSize}px`)
    root.style.setProperty('--editor-line-height', String(lineHeight))
  }
  apply()
  api.configuration.onChange(CONFIG_FONT_SIZE, apply)
  api.configuration.onChange(CONFIG_LINE_HEIGHT, apply)
}

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
    // We listen to the local `files:open` event bus, but also import
    // commentsApi / workspaceStore / filesStore directly — those
    // structural imports require the source plugins to be loaded
    // before us. The dependency chain also keeps boot order sensible
    // (workspace → sidebar → files → comments → editor).
    dependsOn: [
      'nexus.workspace',
      'nexus.files',
      'nexus.comments',
      // Kernel-tier deps (BL-XXX Phase 3.2 — see types/plugin.ts).
      'com.nexus.storage',
      'com.nexus.git',
      'com.nexus.editor',
      'com.nexus.ai',
    ],
    contributes: {
      commands: [
        { id: COMMAND_CLOSE_TAB, title: 'Close Tab', category: 'Editor' },
        { id: COMMAND_SAVE, title: 'Save', category: 'Editor' },
        { id: COMMAND_NEW_UNTITLED, title: 'New Untitled Tab', category: 'Editor' },
        { id: COMMAND_CLOSE_ALL, title: 'Close All Tabs', category: 'Editor' },
        { id: COMMAND_TOGGLE_MODE, title: 'Toggle Source / Live', category: 'Editor' },
        { id: COMMAND_TOGGLE_READING_VIEW, title: 'Toggle reading view', category: 'Editor' },
        { id: COMMAND_FIND, title: 'Find', category: 'Editor' },
        { id: COMMAND_REPLACE, title: 'Replace', category: 'Editor' },
        { id: COMMAND_COPY_REL_PATH, title: 'Copy Path (relative)', category: 'Editor' },
        { id: COMMAND_COPY_ABS_PATH, title: 'Copy Path (absolute)', category: 'Editor' },
        { id: COMMAND_REVEAL_IN_NAV, title: 'Reveal File in Navigation', category: 'Editor' },
        { id: COMMAND_REVEAL_IN_OS, title: 'Show in System Explorer', category: 'Editor' },
        { id: COMMAND_OPEN_DEFAULT_APP, title: 'Open in Default App', category: 'Editor' },
        { id: COMMAND_DELETE_FILE, title: 'Delete File', category: 'Editor' },
        { id: COMMAND_TOGGLE_BLAME, title: 'Toggle Inline Git Blame', category: 'Editor' },
        { id: COMMAND_OPEN_DIFF, title: 'Open Diff for Active File', category: 'Editor' },
        { id: COMMAND_LSP_RENAME, title: 'Rename Symbol (LSP)', category: 'Editor' },
        { id: COMMAND_LSP_CODE_ACTIONS, title: 'Code Actions (LSP)', category: 'Editor' },
        // BL-141 Phase 3 — LSP results in the multibuffer view.
        {
          id: COMMAND_LSP_FIND_REFERENCES,
          title: 'Find All References (LSP)',
          category: 'Editor',
        },
        {
          id: COMMAND_LSP_RENAME_PREVIEW,
          title: 'Rename Symbol — Preview (LSP)',
          category: 'Editor',
        },
        ...STUB_COMMANDS.map((s) => ({ id: s.id, title: s.title, category: 'Editor' })),
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
        // BL-077 follow-up — VS Code muscle memory. F2 has no
        // platform-specific override (mac uses Fn+F2 by default but
        // the bare `F2` is the convention even on macOS).
        {
          command: COMMAND_LSP_RENAME,
          key: 'F2',
          when: CONTEXT_KEY_HAS_ACTIVE_TAB,
        },
        // BL-077 follow-up — Mod-. matches VS Code's "Quick Fix" chord.
        {
          command: COMMAND_LSP_CODE_ACTIONS,
          key: 'ctrl+.',
          mac: 'cmd+.',
          when: CONTEXT_KEY_HAS_ACTIVE_TAB,
        },
        // BL-141 Phase 3 — Shift-F12 matches VS Code's
        // "Find All References" binding.
        {
          command: COMMAND_LSP_FIND_REFERENCES,
          key: 'shift+F12',
          when: CONTEXT_KEY_HAS_ACTIVE_TAB,
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
      // BL-142 Phase 3 — REPL Kernels settings tab.
      settingsTabs: [
        {
          id: 'editor.repl-kernels',
          title: 'REPL Kernels',
          group: 'options',
          priority: 55,
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
    installMarginSuggestStyles()
    // R10 / #193 — publish the editor surface the host's `api.editor`
    // delegates to. The host owns the seam; this plugin owns the store,
    // projection, and de-dup. Registered before any consumer plugin
    // (mermaid, enrich, …) activates, since the editor is a core plugin.
    // The returned disposer is discarded: the surface lives for the
    // plugin's lifetime, matching every other long-lived registration in
    // this file (no plugin-deactivate hook exists yet).
    void registerEditorHostSurface({
      getActiveEditor: () => computeActiveEditor(useEditorStore.getState()),
      subscribeActiveEditor: (handler) => {
        let last = computeActiveEditor(useEditorStore.getState())
        return useEditorStore.subscribe((state) => {
          const next = computeActiveEditor(state)
          if (activeEditorEquals(next, last)) return
          last = next
          handler(next)
        })
      },
      registerFencedCodeRenderer: (language, renderer) =>
        fencedCodeRegistry.register(language, renderer),
    })
    // BL-141 / Phase 4.6 — multibuffer external-edit sync. Was a
    // standalone nexus.multibufferSync plugin; folded in here since
    // no other plugin consumed it. Wiring lives in editor/multibuffer/.
    startMultibufferSync(api)
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
        const snapshot = await sessionManager.acquire(relpath)
        if (snapshot === null) {
          // No kernel session: the file is missing / unreadable (e.g. a
          // restored tab from another vault) and `acquire` already
          // degraded to `null`. Calling `getMarkdown` here would just
          // fail with "no open session" — a doomed IPC round-trip the
          // kernel logs. Surface the load failure directly instead.
          useEditorStore
            .getState()
            .setTabError(relpath, `cannot open '${relpath}' (no session)`)
          return
        }
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

      // openTab seeds new tabs in 'live' mode; honour the user's
      // default-mode preference if they've flipped it.
      const defaultMode = api.configuration.getValue<string>(CONFIG_DEFAULT_MODE, 'live')
      if (defaultMode === 'source' || defaultMode === 'preview') {
        useEditorStore.getState().setMode(payload.relpath, defaultMode)
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
      // BL-142 Phase 2b.1 — tear down any REPL sessions tagged to
      // this relpath so they don't leak on tab close. Best-effort:
      // a transport error during teardown is swallowed by
      // `stopForTab`, the store entry is cleared regardless so the
      // tab-reopen path gets a clean slate.
      void useReplStore
        .getState()
        .stopForTab(makeReplClient(api.kernel), relpath)
    }

    // Phase 7: legacy SlotRegistry slot:'editorArea' entry removed.
    // `.md` opens now land as leaves of type 'markdown' in the main dock.
    api.viewRegistry.register(
      'markdown',
      markdownViewCreator(
        (relpath, leafId) => createElement(EditorView, { relpath, leafId, onRetry: handleRetry }),
        sessionManager,
      ),
    )
    // P2-03 — override via `nexus.editor.fileExtensions` (string[] of
    // bare extensions, no leading dot). The default `['md', 'markdown']`
    // matches both the CommonMark `.md` and the rarer-but-supported
    // `.markdown` ending.
    const markdownExtensions = api.configuration.getValue<string[]>(
      'nexus.editor.fileExtensions',
      ['md', 'markdown'],
    )
    api.viewRegistry.registerExtensions(markdownExtensions, 'markdown')
    // Code-mode files share the same view type — the editor picks
    // document vs code rendering at tab-open time based on the
    // extension. Without this, opening `.toml` / `.rs` / etc. falls
    // through to the empty-view fallback even though the editor knows
    // how to render them. User-added extensions in
    // `nexus.editor.codeFileExtensions` aren't covered here yet — that's
    // a separate enhancement (the registry doesn't currently re-bind
    // on config change).
    api.viewRegistry.registerExtensions([...DEFAULT_CODE_EXTENSIONS], 'markdown')

    // Override the default no-op empty view (shell/src/workspace/ViewRegistry.ts)
    // with one that renders the Obsidian-style action links — used by
    // the tab-strip `+` button and any other leaf that lands on the
    // empty type (e.g. restored placeholder leaves).
    api.viewRegistry.update(EMPTY_VIEW_TYPE, emptyViewCreator)

    // Settings panel auto-generates UI from this. Defaults match the
    // pre-settings behaviour so existing users don't see a regression.
    api.configuration.register({
      pluginId: 'nexus.editor',
      title: 'Editor',
      order: 10,
      category: 'editor',
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
            'Whether newly-opened markdown files start in WYSIWYG live preview, raw source, or rendered reading view. Read at tab-open time.',
          type: 'select',
          default: 'live',
          options: ['live', 'source', 'preview'],
        },
        {
          key: CONFIG_KEYBINDINGS,
          title: 'Keybindings',
          description:
            "Keymap layered over the default markdown bindings. 'vim' enables Normal/Insert/Visual modes plus :w / :q ex commands. 'emacs' adds C-f/b/n/p navigation, M-f/b word motion, C-Space mark ring, and a kill ring (C-k / C-w / M-w / C-y). Applied at tab-open time — close and reopen the tab after changing this.",
          type: 'select',
          default: 'default',
          options: ['default', 'vim', 'emacs'],
        },
        {
          key: CONFIG_CODE_FILE_EXTENSIONS,
          title: 'Code-mode file extensions',
          description:
            "Comma-separated list of file extensions that open in code mode (raw CodeMirror with a language extension) rather than document mode. Markdown is always document-mode regardless of this list. Default covers Rust, TypeScript, JavaScript, Python, Go, JSON, YAML, and TOML.",
          type: 'string',
          default: 'rs,ts,tsx,js,jsx,mjs,cjs,py,go,json,jsonc,yaml,yml,toml',
        },
        {
          key: CONFIG_FONT_SIZE,
          title: 'Font size',
          description: 'Editor font size in pixels for source and code modes. Applied live.',
          type: 'number',
          default: 13,
        },
        {
          key: CONFIG_LINE_HEIGHT,
          title: 'Line height',
          description: 'Unitless line-height multiplier for editor content. Applied live.',
          type: 'number',
          default: 1.6,
        },
        {
          key: CONFIG_LINE_NUMBERS,
          title: 'Show line numbers',
          description: 'Show a gutter with line numbers in source and code modes. Applied live to open tabs.',
          type: 'boolean',
          default: false,
        },
        {
          key: CONFIG_EDIT_PREDICTION_ENABLED,
          title: 'Edit prediction (continuous ghost text)',
          description:
            "BL-139 — show inline AI-suggested completions as ghost text as you type. Off by default to avoid surprise GPU / network usage. Routes through com.nexus.ai::predict; the actual provider is whatever's configured in Settings → AI.",
          type: 'boolean',
          default: false,
        },
        {
          key: CONFIG_EDIT_PREDICTION_DEBOUNCE_MS,
          title: 'Edit prediction debounce (ms)',
          description:
            'Quiet period after a keystroke before firing a prediction request. Lower = more responsive, more requests. Default 150ms matches the BL-139 DoD.',
          type: 'number',
          default: 150,
        },
        {
          key: CONFIG_EDIT_PREDICTION_PROVIDER,
          title: 'Edit prediction provider (informational)',
          description:
            "The AI plugin's configured provider drives routing. Set this only as a label for documentation/UI — changing it does not change the actual provider.",
          type: 'select',
          default: 'ollama',
          options: ['ollama', 'openai', 'anthropic'],
        },
        {
          key: CONFIG_EDIT_PREDICTION_MODEL,
          title: 'Edit prediction model (informational)',
          description:
            'Model identifier the AI plugin should use for predictions. Same routing caveat as the provider field — informational only.',
          type: 'string',
          default: 'qwen2.5-coder:7b',
        },
        {
          key: CONFIG_REPL_KERNELS,
          title: 'REPL kernels (BL-142)',
          description:
            'JSON map from language tag to the kernel command string used by REPL-marked code blocks (` ```python repl `). Example: `{"python":"python3 -i","node":"node --interactive"}`. Default `{}` — opt-in: REPL execution is inert until at least one kernel is configured. Phase 2b lands the Run gutter + Shift-Enter binding that consumes this.',
          type: 'string',
          default: REPL_KERNELS_DEFAULT_JSON,
        },
      ],
    })

    // BL-142 Phase 3 — friendly editor over the JSON-string
    // `nexus.editor.replKernels` schema entry above. The JSON schema
    // remains the source of truth; this tab is a polish surface.
    api.settings.registerTab('editor.repl-kernels', ReplKernelsTab, {
      title: 'REPL Kernels',
      group: 'options',
      priority: 55,
    })

    applyEditorCssVars(api)

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
      // Cycle live ↔ source. Preview is reachable via the reading-view
      // command / more-menu only — a user currently in preview is
      // routed back into live so this command always lands them on an
      // editable surface.
      const next: EditorTabMode = tab.mode === 'source' ? 'live' : 'source'
      s.setMode(tab.relpath, next)
    })

    api.commands.register(COMMAND_TOGGLE_READING_VIEW, async () => {
      const s = useEditorStore.getState()
      const tab = s.tabs.find((t) => t.relpath === s.activeRelpath)
      if (!tab) return
      // Flip live ↔ preview. From source, route to preview so the user
      // exits the raw editing surface into the rendered view as the
      // command title implies.
      const next: EditorTabMode = tab.mode === 'preview' ? 'live' : 'preview'
      s.setMode(tab.relpath, next)
    })

    // BL-079 — flip the inline-blame extension on / off for the
    // editor view as a whole. The store is global so a flip
    // affects every open tab; the existing CM remount key picks
    // up the change.
    api.commands.register(COMMAND_TOGGLE_BLAME, async () => {
      useEditorBlameStore.getState().toggle()
    })

    // BL-079 — modal diff viewer for the active tab. Mounts a
    // detached div + React root that survives until the user
    // dismisses the modal; cleaning up on close avoids growing a
    // pile of stale roots across repeat opens.
    let openDiffRoot: { el: HTMLElement; root: Root } | null = null
    const closeDiffRoot = () => {
      if (!openDiffRoot) return
      const { el, root } = openDiffRoot
      try {
        root.unmount()
      } catch {
        // Best-effort: a double-close shouldn't surface as an error
        // to the user.
      }
      el.remove()
      openDiffRoot = null
    }
    api.commands.register(COMMAND_OPEN_DIFF, async () => {
      const s = useEditorStore.getState()
      const tab = s.tabs.find((t) => t.relpath === s.activeRelpath)
      if (!tab || /^untitled-\d+$/i.test(tab.relpath)) return
      // Replace any prior modal first — keeps the markup tidy if
      // the user invokes the command twice.
      closeDiffRoot()
      const el = document.createElement('div')
      el.className = 'nexus-diff-view-host'
      document.body.appendChild(el)
      const root = createRoot(el)
      openDiffRoot = { el, root }
      root.render(
        createElement(DiffView, {
          kernel: api.kernel,
          relpath: tab.relpath,
          onClose: closeDiffRoot,
        }),
      )
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

    // BL-077 follow-up — LSP rename. Bound to F2 on every focused
    // editor; gracefully degrades when the active tab isn't a
    // code-mode CM6 view, when the LSP server doesn't support
    // `textDocument/rename`, or when the cursor isn't on a renamable
    // symbol. The applier handles multi-file edits transparently.
    const lspIpc = new LspIpc(api.kernel)
    api.commands.register(COMMAND_LSP_RENAME, async () => {
      const view = getActiveCmView()
      const relpath = activeTabRelpath()
      if (!view || !relpath) {
        api.notifications.show({
          type: 'info',
          message: 'Rename requires a code-mode editor tab.',
        })
        return
      }
      const sel = view.state.selection.main
      const lineInfo = view.state.doc.lineAt(sel.head)
      const line = lineInfo.number - 1
      const character = sel.head - lineInfo.from

      // Pre-fill the prompt with the word at cursor — saves a
      // round-trip to `prepareRename` and matches user expectation
      // (VS Code does the same when the LSP server doesn't return a
      // prepare result).
      const wordRegex = /[A-Za-z_$][\w$]*/
      let defaultName = ''
      const beforeOnLine = view.state.doc
        .sliceString(lineInfo.from, sel.head)
        .match(/[A-Za-z_$][\w$]*$/)
      const afterOnLine = view.state.doc
        .sliceString(sel.head, lineInfo.to)
        .match(/^[\w$]*/)
      if (beforeOnLine || afterOnLine) {
        defaultName = `${beforeOnLine?.[0] ?? ''}${afterOnLine?.[0] ?? ''}`
      }

      const newName = await api.input.prompt(
        defaultName ? `Rename "${defaultName}" to:` : 'Rename symbol to:',
        defaultName,
      )
      if (!newName || !wordRegex.test(newName)) {
        if (newName != null) {
          api.notifications.show({
            type: 'warning',
            message: 'Rename cancelled — the new name is not a valid identifier.',
          })
        }
        return
      }

      let raw: unknown
      try {
        raw = await lspIpc.rename({
          path: relpath,
          line,
          character,
          new_name: newName,
        })
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Rename failed: ${err instanceof Error ? err.message : String(err)}`,
        })
        return
      }
      if (!raw) {
        api.notifications.show({
          type: 'info',
          message: 'No rename available at this position.',
        })
        return
      }

      const forgeRoot = useWorkspaceStore.getState().rootPath
      if (!forgeRoot) {
        api.notifications.show({
          type: 'error',
          message: 'Rename failed: no workspace open.',
        })
        return
      }

      try {
        const result = await applyWorkspaceEdit(raw as LspWorkspaceEdit, {
          forgeRoot,
          activeView: view,
          activeRelpath: relpath,
          readFile: async (p) => {
            const resp = await api.kernel.invoke<ReadFileResponse>(
              STORAGE_PLUGIN_ID,
              READ_FILE_COMMAND,
              { path: p },
            )
            return decodeUtf8(resp.bytes ?? [])
          },
          writeFile: writeStorageFile,
          onSkip: (uri, reason) => {
            clientLogger.warn(
              '[nexus.editor.lsp.rename] skipped URI outside forge:',
              uri,
              reason,
            )
          },
        })
        const total = result.liveViewFiles + result.storageFiles
        if (total === 0 && result.skipped.length === 0) {
          api.notifications.show({
            type: 'info',
            message: 'Rename returned no edits.',
          })
          return
        }
        const skipNote =
          result.skipped.length > 0
            ? ` (${result.skipped.length} outside-forge URI${
                result.skipped.length === 1 ? '' : 's'
              } skipped)`
            : ''
        api.notifications.show({
          type: 'info',
          message: `Renamed in ${total} file${total === 1 ? '' : 's'}${skipNote}.`,
        })
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Rename apply failed: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })

    // BL-141 Phase 3 — find-all-references → multibuffer. Reads
    // the cursor position, calls `lspIpc.references`, converts the
    // `Location[]` response into excerpts (with 3 lines of context
    // around each match), opens a synthetic session via
    // `editorClient.openExcerpts`, and routes the resulting
    // `multibuffer://<uuid>` relpath into the standard `files:open`
    // pipeline so the new tab mounts. Re-opening that relpath from
    // the editor's session-acquire path lands on the idempotent
    // synthetic-open codepath (no disk read).
    api.commands.register(COMMAND_LSP_FIND_REFERENCES, async () => {
      const view = getActiveCmView()
      const relpath = activeTabRelpath()
      if (!view || !relpath) {
        api.notifications.show({
          type: 'info',
          message: 'Find references requires a code-mode editor tab.',
        })
        return
      }
      const forgeRoot = useWorkspaceStore.getState().rootPath
      if (!forgeRoot) {
        api.notifications.show({
          type: 'error',
          message: 'Find references failed: no workspace open.',
        })
        return
      }
      const sel = view.state.selection.main
      const lineInfo = view.state.doc.lineAt(sel.head)
      const line = lineInfo.number - 1
      const character = sel.head - lineInfo.from

      let raw: unknown
      try {
        raw = await lspIpc.references({
          path: relpath,
          line,
          character,
          include_declaration: true,
        })
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Find references failed: ${err instanceof Error ? err.message : String(err)}`,
        })
        return
      }
      const locations = (raw ?? []) as LspLocation[]
      const items = locationsToExcerptRequests(locations, { forgeRoot })
      if (items.length === 0) {
        api.notifications.show({
          type: 'info',
          message: 'No references found in the forge.',
        })
        return
      }
      try {
        const snap = await editorClient.openExcerpts(items)
        api.events.emit('files:open', {
          relpath: snap.relpath,
          name: `References (${items.length})`,
        })
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Open references view failed: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })

    // BL-141 Phase 3 — rename preview → multibuffer. Same prompt
    // as `COMMAND_LSP_RENAME`, but instead of applying the
    // WorkspaceEdit immediately, opens the affected ranges in a
    // multibuffer so the user can scan / edit the proposed changes
    // before committing. The user can dispatch the apply path
    // (`COMMAND_LSP_RENAME`) separately, or save the multibuffer
    // (Phase 2 Approach A wires multibuffer save to per-source
    // splice writes).
    api.commands.register(COMMAND_LSP_RENAME_PREVIEW, async () => {
      const view = getActiveCmView()
      const relpath = activeTabRelpath()
      if (!view || !relpath) {
        api.notifications.show({
          type: 'info',
          message: 'Rename preview requires a code-mode editor tab.',
        })
        return
      }
      const forgeRoot = useWorkspaceStore.getState().rootPath
      if (!forgeRoot) {
        api.notifications.show({
          type: 'error',
          message: 'Rename preview failed: no workspace open.',
        })
        return
      }
      const sel = view.state.selection.main
      const lineInfo = view.state.doc.lineAt(sel.head)
      const line = lineInfo.number - 1
      const character = sel.head - lineInfo.from

      const newName = await api.input.prompt('Preview rename to:', '')
      if (!newName) return

      let raw: unknown
      try {
        raw = await lspIpc.rename({
          path: relpath,
          line,
          character,
          new_name: newName,
        })
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Rename preview failed: ${err instanceof Error ? err.message : String(err)}`,
        })
        return
      }
      if (!raw) {
        api.notifications.show({
          type: 'info',
          message: 'No rename available at this position.',
        })
        return
      }
      const items = workspaceEditToExcerptRequests(
        raw as LspWorkspaceEdit,
        { forgeRoot },
      )
      if (items.length === 0) {
        api.notifications.show({
          type: 'info',
          message: 'Rename returned no in-forge edits to preview.',
        })
        return
      }
      try {
        const snap = await editorClient.openExcerpts(items)
        api.events.emit('files:open', {
          relpath: snap.relpath,
          name: `Rename preview → "${newName}" (${items.length})`,
        })
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Open rename preview failed: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })

    // BL-077 follow-up — LSP code actions. Quick-pick of every action
    // the server returns at the cursor; the chosen action's
    // WorkspaceEdit applies via the same `applyWorkspaceEdit` path
    // as rename. Command-only actions (no `edit`) require LSP
    // `workspace/executeCommand` which the host doesn't expose yet
    // — we surface them as disabled rows so users see them in the
    // list but can't act on them.
    interface LspCodeAction {
      title: string
      kind?: string
      disabled?: { reason: string }
      edit?: LspWorkspaceEdit
      command?: { title: string; command: string; arguments?: unknown[] }
    }
    api.commands.register(COMMAND_LSP_CODE_ACTIONS, async () => {
      const view = getActiveCmView()
      const relpath = activeTabRelpath()
      if (!view || !relpath) {
        api.notifications.show({
          type: 'info',
          message: 'Code actions require a code-mode editor tab.',
        })
        return
      }
      const sel = view.state.selection.main
      const startLine = view.state.doc.lineAt(sel.from)
      const endLine = view.state.doc.lineAt(sel.to)
      const range = {
        start: {
          line: startLine.number - 1,
          character: sel.from - startLine.from,
        },
        end: {
          line: endLine.number - 1,
          character: sel.to - endLine.from,
        },
      }
      let raw: unknown
      try {
        raw = await lspIpc.codeActions({ path: relpath, range })
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Code actions failed: ${err instanceof Error ? err.message : String(err)}`,
        })
        return
      }
      // Reply is `(Command | CodeAction)[] | null`. LSP3.16+ servers
      // emit `CodeAction`; we tolerate the legacy `Command` shape (no
      // `edit`, just a command) by surfacing it as a disabled row
      // alongside actions whose `edit` is missing.
      const actions = (Array.isArray(raw) ? (raw as LspCodeAction[]) : []) ?? []
      if (actions.length === 0) {
        api.notifications.show({
          type: 'info',
          message: 'No code actions available at this position.',
        })
        return
      }
      const pickItems = actions.map((action) => {
        const hasEdit = action.edit != null
        const hasCommand = action.command != null
        const isDisabled = action.disabled != null
        const detail = action.disabled?.reason
          ? `disabled — ${action.disabled.reason}`
          : !hasEdit && hasCommand
            ? `runs server command \`${action.command!.command}\``
            : action.kind
        return {
          label: action.title,
          description: action.kind,
          detail,
          value: { action, isDisabled },
        }
      })
      const picked = await api.input.pick(pickItems, {
        title: 'Code actions',
        placeholder: 'Filter actions…',
      })
      if (!picked) return
      if (picked.isDisabled) {
        api.notifications.show({
          type: 'warning',
          message: `Action disabled: ${picked.action.disabled?.reason ?? 'no reason given'}`,
        })
        return
      }
      const edit = picked.action.edit
      if (!edit) {
        // No `edit` — fall through to `workspace/executeCommand` if
        // the action carries a `command`. Server-driven side effect
        // (apply-import, generate-bindings, etc.); the server may
        // also send a follow-up `workspace/applyEdit` request which
        // the host doesn't yet handle (server-initiated requests are
        // a separate BL-076 follow-up). Today we surface a concise
        // "command dispatched" toast and let the user re-load the
        // file via the watcher if the server modified anything.
        const command = picked.action.command
        if (!command) {
          api.notifications.show({
            type: 'info',
            message: 'Selected action did not return any edits.',
          })
          return
        }
        try {
          await lspIpc.executeCommand({
            path: relpath,
            command: command.command,
            arguments: command.arguments ?? [],
          })
          api.notifications.show({
            type: 'info',
            message: `Dispatched server command "${command.title}".`,
          })
        } catch (err) {
          api.notifications.show({
            type: 'error',
            message: `Server command failed: ${err instanceof Error ? err.message : String(err)}`,
          })
        }
        return
      }

      const forgeRoot = useWorkspaceStore.getState().rootPath
      if (!forgeRoot) {
        api.notifications.show({
          type: 'error',
          message: 'Code action failed: no workspace open.',
        })
        return
      }

      try {
        const result = await applyWorkspaceEdit(edit, {
          forgeRoot,
          activeView: view,
          activeRelpath: relpath,
          readFile: async (p) => {
            const resp = await api.kernel.invoke<ReadFileResponse>(
              STORAGE_PLUGIN_ID,
              READ_FILE_COMMAND,
              { path: p },
            )
            return decodeUtf8(resp.bytes ?? [])
          },
          writeFile: writeStorageFile,
          onSkip: (uri, reason) => {
            clientLogger.warn(
              '[nexus.editor.lsp.codeActions] skipped URI outside forge:',
              uri,
              reason,
            )
          },
        })
        const total = result.liveViewFiles + result.storageFiles
        const skipNote =
          result.skipped.length > 0
            ? ` (${result.skipped.length} outside-forge URI${
                result.skipped.length === 1 ? '' : 's'
              } skipped)`
            : ''
        api.notifications.show({
          type: 'info',
          message:
            total === 0
              ? `Action "${picked.action.title}" produced no edits.`
              : `Applied "${picked.action.title}" to ${total} file${total === 1 ? '' : 's'}${skipNote}.`,
        })
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Code action apply failed: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
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
        await api.platform.shell.openExternal(joinAbsPath(root, relpath))
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
        await api.platform.shell.openExternal(parentDir(joinAbsPath(root, relpath)))
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Reveal failed: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })

    // P4-07 — wire the subset of tab-action stubs that have a thin
    // existing backend. The remaining stubs (splitRight/Down,
    // mergeFile, exportPdf) need either new layout primitives or new
    // heavy libraries (PDF) and remain "coming soon" until their
    // dedicated follow-ups land.
    const WIRED: ReadonlySet<string> = new Set([
      'nexus.editor.stub.bookmark',
      'nexus.editor.stub.rename',
      'nexus.editor.stub.backlinksInDocument',
      'nexus.editor.stub.moveTo',
      'nexus.editor.stub.openInNewWindow',
      'nexus.editor.stub.openLinkedView',
      'nexus.editor.stub.versionHistory',
      'nexus.editor.stub.addProperty',
      'nexus.editor.stub.splitRight',
      'nexus.editor.stub.splitDown',
      'nexus.editor.stub.exportPdf',
      'nexus.editor.stub.mergeFile',
    ])
    for (const stub of STUB_COMMANDS) {
      if (WIRED.has(stub.id)) continue
      api.commands.register(stub.id, () => {
        api.notifications.show({
          type: 'info',
          message: `${stub.label} — coming soon.`,
        })
      })
    }

    api.commands.register('nexus.editor.stub.bookmark', async () => {
      const relpath = activeTabRelpath()
      if (!relpath) return
      // Delegates to the bookmarks plugin's toggle command so a single
      // implementation drives both the activity-bar pane and the tab
      // context-menu entry.
      await api.commands.execute('nexus.bookmarks.toggleActive')
      // Surface confirmation since the bookmark pane may be closed.
      api.notifications.show({
        type: 'info',
        message: `Bookmark toggled for ${relpath}.`,
      })
    })

    api.commands.register('nexus.editor.stub.backlinksInDocument', async () => {
      await api.commands.execute('nexus.backlinks.focus')
    })

    api.commands.register('nexus.editor.stub.rename', async () => {
      const relpath = activeTabRelpath()
      if (!relpath) return
      const oldBase = relpath.includes('/') ? relpath.slice(relpath.lastIndexOf('/') + 1) : relpath
      const dir = relpath.includes('/') ? relpath.slice(0, relpath.lastIndexOf('/') + 1) : ''
      const next = await api.input.prompt(`Rename "${oldBase}" to:`, oldBase)
      if (!next || next === oldBase) return
      const to = dir + next
      try {
        await api.kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'rename_entry', {
          from: relpath,
          to,
        })
        // The editor reacts to filesystem events for the renamed file —
        // closing the old tab here keeps the tab strip tidy. The watcher
        // emits files:open for the new path so the rename feels atomic.
        useEditorStore.getState().closeTab(relpath)
        api.events.emit('files:open', { relpath: to, name: next })
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Rename failed: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })

    api.commands.register('nexus.editor.stub.moveTo', async () => {
      const relpath = activeTabRelpath()
      if (!relpath) return
      // Prompt with the full forge-relative path. Same underlying IPC
      // as rename — `storage::rename_entry` accepts a destination in
      // any directory and creates parent dirs as needed.
      const to = await api.input.prompt(`Move "${relpath}" to forge-relative path:`, relpath)
      if (!to || to === relpath) return
      try {
        await api.kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'rename_entry', {
          from: relpath,
          to,
        })
        const base = to.includes('/') ? to.slice(to.lastIndexOf('/') + 1) : to
        useEditorStore.getState().closeTab(relpath)
        api.events.emit('files:open', { relpath: to, name: base })
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Move failed: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })

    api.commands.register('nexus.editor.stub.openInNewWindow', async () => {
      const relpath = activeTabRelpath()
      if (!relpath) return
      const activeLeafId = workspace.activeLeafId
      if (!activeLeafId) return
      try {
        // Bridge pairs the workspace-store mutation with the OS-side
        // Tauri popout. We import lazily so this module doesn't pay
        // the cost on plugins that never popout.
        const { popoutLeaf } = await import('../../../workspace/popoutWindowBridge')
        await popoutLeaf(activeLeafId, { title: relpath })
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Open in new window failed: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })

    const performSplit = async (direction: 'horizontal' | 'vertical') => {
      const relpath = activeTabRelpath()
      const activeLeafId = workspace.activeLeafId
      if (!activeLeafId) return
      try {
        // Creates a new empty leaf adjacent in the requested direction
        // and makes it active. The subsequent files:open routes into
        // the new leaf because `mountFileInMainLeaf` prefers an empty
        // active leaf in the main dock.
        workspace.splitLeaf(activeLeafId, direction)
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Split failed: ${err instanceof Error ? err.message : String(err)}`,
        })
        return
      }
      if (!relpath) return
      const base = relpath.includes('/') ? relpath.slice(relpath.lastIndexOf('/') + 1) : relpath
      api.events.emit('files:open', { relpath, name: base })
    }

    api.commands.register('nexus.editor.stub.splitRight', async () => {
      await performSplit('horizontal')
    })

    api.commands.register('nexus.editor.stub.splitDown', async () => {
      await performSplit('vertical')
    })

    api.commands.register('nexus.editor.stub.mergeFile', async () => {
      let raw: unknown
      try {
        raw = await api.kernel.invoke<unknown>('com.nexus.git', 'conflict_files', {})
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Conflict lookup failed: ${err instanceof Error ? err.message : String(err)}`,
        })
        return
      }
      const paths = Array.isArray(raw)
        ? (raw as unknown[]).filter((p): p is string => typeof p === 'string')
        : []
      if (paths.length === 0) {
        api.notifications.show({
          type: 'info',
          message: 'No merge conflicts to resolve.',
        })
        return
      }
      const items = paths.map((p) => {
        const base = p.includes('/') ? p.slice(p.lastIndexOf('/') + 1) : p
        return { label: base, description: p, value: p }
      })
      const picked = await api.input.pick<string>(items, {
        title: `Merge conflicts (${paths.length})`,
        placeholder: 'Open conflicted file',
      })
      if (!picked) return
      const base = picked.includes('/') ? picked.slice(picked.lastIndexOf('/') + 1) : picked
      // The editor renders the `<<<<<<<`/`=======`/`>>>>>>>` markers
      // inline. Users resolve manually then stage + commit via the
      // existing git pane. A dedicated 3-pane merge view is the
      // follow-up surface (would mount a CodeMirror merge extension
      // wired to conflict_versions for theirs/ours/resolved).
      api.events.emit('files:open', { relpath: picked, name: base })
    })

    api.commands.register('nexus.editor.stub.exportPdf', async () => {
      const relpath = activeTabRelpath()
      if (!relpath) return
      const store = useEditorStore.getState()
      const tab = store.tabs.find((t) => t.relpath === relpath)
      const previous: EditorTabMode | null = tab?.mode ?? null
      // Switch into preview before printing so the OS print dialog
      // captures rendered Markdown rather than the editor's CodeMirror
      // chrome. The user picks "Save as PDF" from their print dialog.
      if (previous && previous !== 'preview') {
        store.setMode(relpath, 'preview')
      }
      // Wait one frame for the preview to render before opening the dialog.
      await new Promise((resolve) => setTimeout(resolve, 250))
      try {
        window.print()
      } finally {
        if (previous && previous !== 'preview') {
          useEditorStore.getState().setMode(relpath, previous)
        }
      }
    })

    api.commands.register('nexus.editor.stub.addProperty', async () => {
      const relpath = activeTabRelpath()
      if (!relpath) return
      const key = await api.input.prompt('Property key:', 'tags')
      if (!key) return
      const value = await api.input.prompt(`Value for "${key}":`, '')
      if (value === null) return
      try {
        await api.kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'write_frontmatter', {
          path: relpath,
          key,
          value,
        })
        api.notifications.show({
          type: 'info',
          message: `Property "${key}" updated.`,
        })
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Add property failed: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })

    api.commands.register('nexus.editor.stub.versionHistory', async () => {
      const relpath = activeTabRelpath()
      if (!relpath) return
      interface LogEntry {
        hash?: unknown
        author?: unknown
        date?: unknown
        message?: unknown
      }
      let raw: unknown
      try {
        raw = await api.kernel.invoke<unknown>('com.nexus.git', 'file_log', {
          path: relpath,
          limit: 50,
        })
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Version history failed: ${err instanceof Error ? err.message : String(err)}`,
        })
        return
      }
      if (!Array.isArray(raw) || raw.length === 0) {
        api.notifications.show({
          type: 'info',
          message: 'No commit history for this file.',
        })
        return
      }
      const items = (raw as LogEntry[])
        .map((entry) => {
          const hash =
            typeof entry.hash === 'string' ? entry.hash.slice(0, 7) : '???????'
          const date = typeof entry.date === 'string' ? entry.date.slice(0, 10) : ''
          const author = typeof entry.author === 'string' ? entry.author : ''
          const message = typeof entry.message === 'string'
            ? entry.message.split('\n')[0]
            : ''
          return {
            label: `${hash} · ${date} · ${message}`,
            description: author,
            value: typeof entry.hash === 'string' ? entry.hash : '',
          }
        })
        .filter((i) => i.value.length > 0)
      if (items.length === 0) {
        api.notifications.show({ type: 'info', message: 'No commits for this file.' })
        return
      }
      const picked = await api.input.pick<string>(items, {
        title: `History — ${relpath}`,
        placeholder: 'Pick a commit',
      })
      if (!picked) return
      // Surface the commit hash via clipboard + toast — a full diff
      // viewer is the follow-up surface (would split a diff leaf
      // showing this commit vs HEAD).
      try {
        await navigator.clipboard.writeText(picked)
        api.notifications.show({
          type: 'info',
          message: `Commit ${picked.slice(0, 7)} copied to clipboard.`,
        })
      } catch {
        api.notifications.show({
          type: 'info',
          message: `Selected commit ${picked.slice(0, 7)}.`,
        })
      }
    })

    api.commands.register('nexus.editor.stub.openLinkedView', async () => {
      const relpath = activeTabRelpath()
      if (!relpath) return
      let raw: unknown
      try {
        raw = await api.kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'backlinks', { path: relpath })
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Backlinks lookup failed: ${err instanceof Error ? err.message : String(err)}`,
        })
        return
      }
      if (!Array.isArray(raw) || raw.length === 0) {
        api.notifications.show({ type: 'info', message: 'No linked notes.' })
        return
      }
      const seen = new Set<string>()
      const items: { label: string; value: string }[] = []
      for (const row of raw as Array<{ source_path?: unknown }>) {
        const src = typeof row?.source_path === 'string' ? row.source_path : null
        if (!src || src === relpath || seen.has(src)) continue
        seen.add(src)
        const base = src.includes('/') ? src.slice(src.lastIndexOf('/') + 1) : src
        items.push({ label: base, value: src })
      }
      if (items.length === 0) {
        api.notifications.show({ type: 'info', message: 'No linked notes.' })
        return
      }
      const picked = await api.input.pick<string>(items, {
        placeholder: 'Open linked note',
      })
      if (!picked) return
      const base = picked.includes('/') ? picked.slice(picked.lastIndexOf('/') + 1) : picked
      api.events.emit('files:open', { relpath: picked, name: base })
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

    // BL-049 phase 2 — block-link navigation. Click in CM emits
    // `files:open` to raise the target tab and a follow-up
    // `nexus.editor:reveal-block` event the editor plugin
    // consumes after the file finishes loading. The reveal
    // handler searches the doc for the `<!-- ^<uuid> -->`
    // marker and scrolls; if the marker isn't yet on disk
    // (block has been stamped in-memory but the session hasn't
    // saved), the resolver-supplied `root_index` is the
    // fallback — phase-3 work, since per-block source-position
    // metadata isn't on the snapshot today.
    const pendingReveals = new Map<string, string>()

    setEditorRuntime({
      confirmAndClose,
      openUntitled,
      closeAll,
      kernelClient: editorClient,
      sessionManager,
      getKeybindings: () => {
        const v = api.configuration.getValue<string>(CONFIG_KEYBINDINGS, 'default')
        if (v === 'vim') return 'vim'
        if (v === 'emacs') return 'emacs'
        return 'default'
      },
      getCodeFileExtensions: () => {
        // Parse the comma-separated config string into a normalised
        // list. Strip leading dots so users can type either `.rs` or
        // `rs`; lowercase + trim each token so `RS, ts , .py ` all
        // round-trip cleanly. An empty / whitespace-only setting
        // string falls back to the default list rather than
        // disabling code mode entirely — that's almost certainly a
        // settings-UI mistake, not "the user wants no code mode".
        const raw = api.configuration.getValue<string>(
          CONFIG_CODE_FILE_EXTENSIONS,
          '',
        )
        const parsed = raw
          .split(',')
          .map((s) => s.trim().toLowerCase().replace(/^\.+/, ''))
          .filter((s) => s.length > 0)
        if (parsed.length > 0) return parsed
        return DEFAULT_CODE_EXTENSIONS
      },
      reportBridgeError: (message, err) => {
        api.notifications.show({
          type: 'error',
          message: `${message}: ${err instanceof Error ? err.message : String(err)}`,
        })
      },
      kernelEvents: api.kernel,
      kernel: api.kernel,
      // #202 / R12 — inline-toolbar Link button + Mod-k. Sandbox-safe
      // route to the styled prompt modal; `window.prompt` is disabled
      // inside the null-origin iframe used for JS plugins.
      promptForLinkUrl: () => api.input.prompt('Link URL', 'https://'),
      onBlockLinkNavigate: (link) => {
        // Best-effort tab-name guess from the basename — matches
        // what the files plugin emits for `files:open`.
        const lastSlash = Math.max(
          link.filePath.lastIndexOf('/'),
          link.filePath.lastIndexOf('\\'),
        )
        const name = lastSlash >= 0 ? link.filePath.slice(lastSlash + 1) : link.filePath
        // Validate the link via the resolver so a stale id surfaces
        // as a notification instead of silently scrolling nowhere.
        void editorClient
          .resolveBlockLink(link.filePath, link.blockId)
          .then((resp) => {
            if (!resp.found) {
              api.notifications.show({
                type: 'warning',
                message: `Block ${link.blockId.slice(0, 8)}… not found in ${name}`,
              })
              return
            }
            api.events.emit(EVENT_FILE_OPEN, { relpath: link.filePath, name })
            pendingReveals.set(link.filePath, link.blockId)
            api.events.emit('nexus.editor:reveal-block', {
              relpath: link.filePath,
              blockId: link.blockId,
            })
          })
          .catch((err) => {
            api.notifications.show({
              type: 'error',
              message: `Block-link navigation failed: ${err instanceof Error ? err.message : String(err)}`,
            })
          })
      },
    })

    // Reveal pending block-link targets once the underlying
    // CM view exists for the active tab. Two trigger paths
    // exist — `files:open` (already handled above to load the
    // file) and the editor store's active-tab change. We tap
    // the latter via a zustand subscription so a click on a
    // block-link whose tab is *already open* still scrolls.
    const tryReveal = (relpath: string) => {
      const blockId = pendingReveals.get(relpath)
      if (!blockId) return
      // Defer one tick so the EditorView has a chance to mount
      // its CM instance via the active-tab effect.
      queueMicrotask(() => {
        const cm = getActiveCmView()
        if (!cm) return
        const ok = revealBlockInView(cm, blockId)
        if (ok) pendingReveals.delete(relpath)
      })
    }
    api.events.on<{ relpath: string }>('nexus.editor:reveal-block', (payload) => {
      if (!payload?.relpath) return
      // Two attempts — one immediate (file is already open),
      // one after a short delay (file is being loaded). The
      // map entry is dropped on the first successful reveal.
      tryReveal(payload.relpath)
      setTimeout(() => tryReveal(payload.relpath), 80)
      setTimeout(() => tryReveal(payload.relpath), 250)
    })

    // BL-077 follow-up — `nexus.editor:reveal-line` consumer.
    // Cmd+Click → definition emits the event after `files:open`
    // raises the destination tab. Mirror the reveal-block staging
    // — when the receiving tab's CM view isn't mounted yet, queue
    // the (line, character) and replay on the next tick. The
    // queue is keyed by relpath so a second click for the same
    // file overwrites the pending entry rather than queueing
    // both.
    const pendingLineReveals = new Map<
      string,
      { line: number; character: number }
    >()
    const tryRevealLine = (relpath: string) => {
      const target = pendingLineReveals.get(relpath)
      if (!target) return
      // Defer one tick so the EditorView has a chance to mount
      // its CM instance via the active-tab effect (same rationale
      // as `tryReveal` above).
      queueMicrotask(() => {
        const cm = getActiveCmView()
        if (!cm) return
        // Match the active relpath against the queued target so a
        // racing tab switch doesn't scroll the wrong file. We don't
        // have the cm view's relpath directly here, so cross-check
        // via the editor store.
        if (useEditorStore.getState().activeRelpath !== relpath) return
        revealLineInView(cm, target.line, target.character)
        pendingLineReveals.delete(relpath)
      })
    }
    api.events.on<{
      relpath: string
      line: number
      character: number
    }>('nexus.editor:reveal-line', (payload) => {
      if (!payload?.relpath) return
      if (typeof payload.line !== 'number' || typeof payload.character !== 'number') {
        return
      }
      pendingLineReveals.set(payload.relpath, {
        line: payload.line,
        character: payload.character,
      })
      // Same backoff schedule as block-reveal — one immediate
      // attempt covers the already-open case, the deferred attempts
      // cover the just-loaded-via-files:open case where the new
      // CM view mounts a few frames later.
      tryRevealLine(payload.relpath)
      setTimeout(() => tryRevealLine(payload.relpath), 80)
      setTimeout(() => tryRevealLine(payload.relpath), 250)
    })

    // BL-050 Phase 3 — block-handle "Comment" affordance. The bridge
    // resolves the kernel block_id for the CM-block index, stamps it
    // for stable cross-session anchoring (ADR 0017), prompts for the
    // first comment body, and dispatches `commentsApi.createThread`.
    // Coarse mapping: `tree.root_blocks[blockIndex]` matches CM's
    // source-order paragraph scan well for flat docs (paragraphs +
    // headings); nested children of a root collapse into the root for
    // now. Refinement to per-leaf precision lands when the kernel
    // exposes block-offset metadata to the bridge.
    let cachedCommentsApi: ReturnType<typeof createCommentsApi> | null = null
    const commentsApi = () => {
      if (!cachedCommentsApi) cachedCommentsApi = createCommentsApi(api.kernel)
      return cachedCommentsApi
    }
    // BL-048 — drag-to-embed bridge. Resolves the active tab's
    // (relpath, blockId, label) for the dragged block; the
    // canvas-side drop handler reads the typed payload and builds
    // a text node carrying the BL-049 link form. Phase 3 added the
    // async `stamp(blockIndex)` method (promotes a block to a
    // stable UUID + saves, idempotent) plus a private cache so
    // `resolve` returns the stamped id synchronously for dragstart.
    // See `blockRefDragBridge.ts` for the factory + tests.
    setBlockRefDragBridge(
      createBlockRefDragBridge({
        getActiveRelpath: () => useEditorStore.getState().activeRelpath,
        getSnapshot: (relpath) => sessionManager.getSnapshot(relpath),
        client: editorClient,
        warn: (msg, err) => {
          clientLogger.warn(msg, err)
        },
      }),
    )

    setCommentBridge({
      onCommentBlock: (blockIndex) => {
        void (async () => {
          const relpath = useEditorStore.getState().activeRelpath
          if (!relpath) return
          if (!isMarkdownPath(relpath)) {
            api.notifications.show({
              type: 'info',
              message: 'Comments are only supported on markdown files.',
            })
            return
          }
          if (/^untitled-\d+$/i.test(relpath)) {
            api.notifications.show({
              type: 'info',
              message: 'Save the file before adding comments.',
            })
            return
          }
          let snapshot = sessionManager.getSnapshot(relpath)
          if (!snapshot) {
            try {
              snapshot = await editorClient.getTree(relpath)
            } catch (err) {
              api.notifications.show({
                type: 'error',
                message: `Could not read editor session: ${
                  err instanceof Error ? err.message : String(err)
                }`,
              })
              return
            }
          }
          const roots = snapshot.tree.root_blocks
          if (roots.length === 0) {
            api.notifications.show({
              type: 'warning',
              message: 'Document is empty — no block to comment on.',
            })
            return
          }
          const targetIdx = blockIndex < roots.length ? blockIndex : 0
          const blockId = roots[targetIdx]
          let body: string | null
          try {
            body = await api.input.prompt('Add a comment', '')
          } catch {
            return
          }
          if (body === null) return
          const trimmed = body.trim()
          if (trimmed.length === 0) return
          try {
            const stamp = await editorClient.stampBlock(relpath, blockId)
            await commentsApi().createThread({
              filePath: relpath,
              blockId: stamp.stable_id,
              body: trimmed,
            })
            // Save so the stamp anchor (`<!-- ^<uuid> -->`) is persisted.
            // Without this, a fresh session re-parses the markdown
            // without the marker and the thread orphans on next reopen.
            try {
              await editorClient.saveSession(relpath)
              useEditorStore.getState().markSaved(relpath)
            } catch {
              // Save failures are surfaced via the regular save path's
              // notification machinery; the thread itself was created
              // successfully so don't double-notify here.
            }
            api.events.emit('nexus.comments:reload', { relpath })
            api.notifications.show({
              type: 'info',
              message: 'Comment added.',
            })
          } catch (err) {
            api.notifications.show({
              type: 'error',
              message: `Comment failed: ${err instanceof Error ? err.message : String(err)}`,
            })
          }
        })()
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
          //
          // First push CM's authoritative markdown into the kernel via
          // `sync_content`. The transaction bridge tries hard to keep
          // the kernel tree in sync with CM through `apply_transaction`
          // ops, but any op the translator can't safely express (block-
          // merging backspace, edits inside inline-formatted spans,
          // etc.) leaves CM ahead of the kernel. Without this push,
          // `save` would write the kernel's pre-divergence state and
          // silently lose the user's edits.
          //
          // After sync_content the kernel re-parses and assigns fresh
          // block IDs, so we reset the bridge's optimistic mirror —
          // any queued chain entry against the old IDs short-circuits
          // and the next user keystroke re-translates against the
          // refreshed tree.
          const t0 = performance.now()
          clientLogger.info(
            `[editor.save] start relpath=${tab.relpath} contentLen=${tab.content.length}`,
          )
          await editorClient.syncContent(tab.relpath, tab.content)
          const tSync = performance.now()
          clientLogger.info(
            `[editor.save] syncContent done in ${Math.round(tSync - t0)}ms`,
          )
          sessionManager.resetBridge(tab.relpath)
          try {
            const fresh = await editorClient.getTree(tab.relpath)
            sessionManager.setSnapshot(tab.relpath, fresh)
            // Push the post-sync revision into the store *now* rather
            // than waiting for the async `changed` event from sync_content
            // to land. `markSaved` snapshots `sessionRevision` into
            // `savedRevision`, so if the event arrives after markSaved
            // the tab would stay dirty (`savedRevision = K` but
            // `sessionRevision` later becomes `K+1`).
            useEditorStore
              .getState()
              .setSessionRevision(tab.relpath, fresh.revision)
          } catch {
            // get_tree is a defensive freshness pull; if it fails the
            // next bridge lazy-init will re-fetch via openSession's
            // cached snapshot. Save still proceeds.
          }
          const tGet = performance.now()
          clientLogger.info(
            `[editor.save] resetBridge+getTree done in ${Math.round(tGet - tSync)}ms`,
          )
          await editorClient.saveSession(tab.relpath)
          const tSave = performance.now()
          clientLogger.info(
            `[editor.save] saveSession done in ${Math.round(tSave - tGet)}ms (total ${Math.round(tSave - t0)}ms)`,
          )
          useEditorStore.getState().markSaved(tab.relpath)
          // BL-045 — broadcast a save event so opt-in features
          // (auto-enrichment, etc.) can react. Payload is intentionally
          // tiny: just the forge-relative path of the saved file.
          api.events.emit('files:saved', { relpath: tab.relpath })
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
          api.events.emit('files:saved', { relpath: newRelpath })
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

        // Non-markdown named file — code-mode tab. Run any
        // registered LSP format-on-save hook *before* writing so
        // `tab.content` reflects the post-format buffer (the hook
        // applies edits to the live CM6 view, which the editor
        // store mirrors via the existing change-tracking
        // pipeline). Hook errors are swallowed by `runSaveFormatHook`;
        // we surface them via the bridge-error channel so a broken
        // formatter doesn't silently no-op.
        await runSaveFormatHook(tab.relpath, (err) => {
          api.notifications.show({
            type: 'warning',
            message: `Format-on-save failed: ${
              err instanceof Error ? err.message : String(err)
            }`,
          })
        })
        // Re-read tab.content after format — the hook may have
        // mutated the view, and `useEditorStore` updates its
        // `content` mirror via the editor's change pipeline before
        // returning. Look up the freshest snapshot rather than
        // closing over the stale `tab` reference.
        const fresh =
          useEditorStore.getState().tabs.find((t) => t.relpath === tab.relpath) ?? tab
        await writeStorageFile(fresh.relpath, fresh.content)
        useEditorStore.getState().markSaved(fresh.relpath)
        api.events.emit('files:saved', { relpath: fresh.relpath })
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
    // Diagnostic: log every change to the markdown tab's content/
    // revision state so we can see whether setContent + the bridge
    // are actually reaching the store. Runs outside React so it
    // can't trigger render loops. Remove once dirty-dot wiring is
    // confirmed.
    let lastDiagSummary = ''
    useEditorStore.subscribe((state) => {
      const tab = state.tabs.find((t) =>
        t.relpath.endsWith('AI-MEMORY-LAYER-PLAN.md'),
      )
      if (!tab) return
      const summary = JSON.stringify({
        contentLen: tab.content.length,
        savedContentLen: tab.savedContent.length,
        sessionRev: state.sessionRevision.get(tab.relpath) ?? null,
        savedRev: state.savedRevision.get(tab.relpath) ?? null,
      })
      if (summary === lastDiagSummary) return
      lastDiagSummary = summary
      clientLogger.info(`[editor.store:diag] relpath=${tab.relpath} ${summary}`)
    })

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
          // BL-142 Phase 2b.1 — same teardown as confirmAndClose;
          // covers tabs that vanish because the workspace dropped
          // their leaf (e.g. forge switch) rather than via the ×
          // button.
          void useReplStore
            .getState()
            .stopForTab(makeReplClient(api.kernel), tab.relpath)
        }
      }

      // Main-dock floor: if the user just closed the last main-dock leaf,
      // seed a fresh empty placeholder so the dock is never blank. Side
      // docks are intentionally excluded — collapsing an empty sidedock
      // is a feature, not a bug.
      const mainLeaves: Leaf[] = []
      collectMainLeaves(workspace.rootSplit, mainLeaves)
      if (mainLeaves.length === 0) {
        const target = findFirstMainTabs(workspace.rootSplit)
        if (target) {
          const leaf = workspace.createLeaf(target)
          target.leaves.push(leaf)
          target.activeIndex = target.leaves.length - 1
          void leaf.setViewState({ type: EMPTY_VIEW_TYPE, active: true })
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
      // Pass the snapshots explicitly — `isDirty(tab)` without an
      // explicit source falls back to live store state, which would
      // make `dirty` and `prevDirty` read the same maps and never
      // diverge. The kernel-revision dirty path needs the prev/current
      // revision maps to compare.
      const dirty = !!activeTab && isDirty(activeTab, state)
      const prevDirty = !!prevActiveTab && isDirty(prevActiveTab, prev)
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

    // BL-142 Phase 2b.2 — single bus subscriber that routes
    // `com.nexus.terminal.output.<sessionId>` events into the
    // per-cell `useReplOutputStore`. The widget below each REPL
    // cell subscribes to the store directly; this pump is the
    // single bridge between the bus and the store, started once
    // at activation. The returned `stop` is kept in module scope
    // so a future plugin-deactivate hook (when one lands) can
    // call it; for now the subscription lives for the plugin's
    // lifetime, which matches every other long-lived `api.on`
    // subscription in this file.
    void startReplOutputPump(api.kernel).catch((err) => {
      clientLogger.warn(`[editor.repl] output pump failed to start: ${err}`)
    })
  },
}
