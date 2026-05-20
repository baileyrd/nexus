import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { clientLogger } from '../../../clientLogger'
import { workspace } from '../../../workspace'
import { FilesTree } from './FilesTree'
import { fileExplorerViewCreator } from './FileExplorerView'
import { useFilesStore, type FilesDirEntry } from './filesStore'
import {
  createDir,
  createFile,
  deleteEntry,
  loadChildren,
  renameEntry,
  setKernel,
} from './kernelClient'
import { setApi } from './runtime'
import { useWorkspaceStore } from '../workspace/workspaceStore'
import { useStatusStore } from './status/statusStore'

const COMMAND_FOCUS = 'nexus.files.focus'
// WI-21: context-menu / shortcut commands. Surface the same actions
// the legacy shell's FileTree.tsx exposed via right-click; bind
// Del + F2 to delete/rename for keyboard parity.
const COMMAND_CREATE_FILE = 'nexus.files.create.file'
const COMMAND_CREATE_FOLDER = 'nexus.files.create.folder'
const COMMAND_RENAME = 'nexus.files.rename'
const COMMAND_DELETE = 'nexus.files.delete'
const COMMAND_REVEAL = 'nexus.files.reveal'
const COMMAND_COPY_PATH = 'nexus.files.copyPath'
const CONTEXT_KEY_FOCUSED = 'nexus.files.focused'
const EVENT_FILE_OPEN = 'files:open'
const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

// Topic strings verified against crates/nexus-storage/src/core_plugin.rs::publish_event.
const TOPIC_FILE_CREATED = 'com.nexus.storage.file_created'
const TOPIC_FILE_MODIFIED = 'com.nexus.storage.file_modified'
const TOPIC_FILE_DELETED = 'com.nexus.storage.file_deleted'
const TOPIC_FILE_RENAMED = 'com.nexus.storage.file_renamed'

/**
 * Compute the parent directory relpath for `p`. Uses forward-slash
 * semantics — the storage plugin always emits forge-relative paths
 * with `/` separators, regardless of host OS.
 *
 *   "notes/Ideas/Tasks.md" → "notes/Ideas"
 *   "Welcome.md"           → ""
 *   ""                     → ""
 */
function parentRelpath(p: string): string {
  const i = p.lastIndexOf('/')
  return i === -1 ? '' : p.slice(0, i)
}

/**
 * Pull a `path` / `relpath` / `to` string out of a storage-event
 * payload. The Rust side is consistent — `file_created` /
 * `file_modified` / `file_deleted` carry `path`, `file_renamed`
 * carries `from` + `to`. We care about the destination in every
 * case (a rename's `from` parent invalidates separately if still
 * cached).
 */
function payloadPaths(payload: unknown): string[] {
  if (!payload || typeof payload !== 'object') return []
  const p = payload as Record<string, unknown>
  const out: string[] = []
  const push = (v: unknown) => {
    if (typeof v === 'string') out.push(v)
  }
  push(p.path)
  push(p.relpath)
  push(p.from)
  push(p.to)
  return out
}

export const filesPlugin: Plugin = {
  manifest: {
    id: 'nexus.files',
    name: 'Files',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'com.nexus.storage'],
    contributes: {
      commands: [
        { id: COMMAND_FOCUS, title: 'Focus Files', category: 'Files' },
        { id: COMMAND_CREATE_FILE, title: 'New File', category: 'Files' },
        { id: COMMAND_CREATE_FOLDER, title: 'New Folder', category: 'Files' },
        { id: COMMAND_RENAME, title: 'Rename', category: 'Files' },
        { id: COMMAND_DELETE, title: 'Delete', category: 'Files' },
        { id: COMMAND_REVEAL, title: 'Reveal in OS', category: 'Files' },
        { id: COMMAND_COPY_PATH, title: 'Copy Path', category: 'Files' },
      ],
      keybindings: [
        // Gated by `nexus.files.focused` so Del / F2 only fire when
        // the tree itself owns focus — typing in the editor (or any
        // other input) keeps its native behavior.
        { command: COMMAND_DELETE, key: 'Delete', when: CONTEXT_KEY_FOCUSED },
        { command: COMMAND_RENAME, key: 'F2', when: CONTEXT_KEY_FOCUSED },
      ],
      contextKeys: [
        {
          key: CONTEXT_KEY_FOCUSED,
          description: 'True when the Files tree has DOM focus.',
          type: 'boolean',
        },
      ],
    },
  },

  async activate(api: PluginAPI) {
    setKernel(api.kernel)
    setApi(api)

    const handleFileActivate = (entry: FilesDirEntry) => {
      api.events.emit(EVENT_FILE_OPEN, {
        relpath: entry.relpath,
        name: entry.name,
      })
    }

    // Phase 7 (leaf-migration-plan.md): the legacy SlotRegistry
    // registration for slot:'sidebarContent' was removed. The tree now
    // mounts through the Leaf/View pipeline below.
    api.viewRegistry.register(
      'file-explorer',
      fileExplorerViewCreator(() =>
        createElement(FilesTree, { onFileActivate: handleFileActivate }),
      ),
    )

    // Files view is reached via the sidebar tab strip's folder icon
    // (rendered by WorkspaceRenderer for sidedock leaves). No separate
    // activity-bar entry — it would duplicate the sidebar tab.

    // Focus command — ensure a file-explorer leaf exists in the left
    // sidedock and reveal it. Existence/visibility split follows
    // docs/leaf-migration-plan.md §Resolved decision #2.
    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType('file-explorer', 'left')
      workspace.revealLeaf(leaf)
    })

    // Initialize the focus context key — set true/false from FilesTree's
    // mount/focus/blur handlers (api.context.set(CONTEXT_KEY_FOCUSED, …)).
    api.context.set(CONTEXT_KEY_FOCUSED, false)

    // ── Power-user commands (WI-21) ───────────────────────────────────────
    //
    // Each command accepts an optional explicit `relpath` argument from
    // the right-click menu (which knows what was clicked), and otherwise
    // falls back to `useFilesStore.getState().selected` so the keyboard
    // shortcuts (Del, F2) operate on the highlighted row.
    //
    // Commands that *create* (file/folder) accept an optional `parent`
    // arg; with no arg they nest under the selected dir (or the
    // selected file's parent, or the root).

    /** Forge-relative parent of a relpath. `""` → `""`. */
    const parentRel = (rel: string): string => {
      const i = rel.lastIndexOf('/')
      return i === -1 ? '' : rel.slice(0, i)
    }

    /** Last path segment, e.g. `"a/b/c.md"` → `"c.md"`. */
    const basename = (rel: string): string => {
      const i = rel.lastIndexOf('/')
      return i === -1 ? rel : rel.slice(i + 1)
    }

    /** Walk the cached tree to find an entry by relpath. Returns null
     *  when any segment along the path hasn't been listed yet. */
    const findEntry = (relpath: string): FilesDirEntry | null => {
      if (!relpath) return null
      const cache = useFilesStore.getState().children
      const root = cache['']
      if (!root) return null
      const segments = relpath.split('/')
      let current: FilesDirEntry[] | undefined = root
      let path = ''
      for (let i = 0; i < segments.length; i++) {
        if (!current) return null
        const next = current.find((e) => e.name === segments[i])
        if (!next) return null
        if (i === segments.length - 1) return next
        path = path ? `${path}/${segments[i]}` : segments[i]
        current = cache[path]
      }
      return null
    }

    /** Resolve the "target dir for new entries" given an optional explicit
     *  parent and the current selection. Mirrors FilesTree.parentForNew. */
    const resolveParent = (explicit: string | undefined): string => {
      if (typeof explicit === 'string') return explicit
      const sel = useFilesStore.getState().selected
      if (!sel) return ''
      const entry = findEntry(sel)
      if (entry?.isDir) return entry.relpath
      return parentRel(sel)
    }

    const refreshDir = async (parent: string): Promise<void> => {
      const entries = await loadChildren(parent)
      useFilesStore.getState().setChildren(parent, entries)
    }

    const argRelpath = (args: unknown[]): string | undefined => {
      const a = args[0]
      if (a && typeof a === 'object' && 'relpath' in a) {
        const v = (a as { relpath: unknown }).relpath
        if (typeof v === 'string') return v
      }
      return undefined
    }

    const argParent = (args: unknown[]): string | undefined => {
      const a = args[0]
      if (a && typeof a === 'object' && 'parent' in a) {
        const v = (a as { parent: unknown }).parent
        if (typeof v === 'string') return v
      }
      return undefined
    }

    api.commands.register(COMMAND_CREATE_FILE, async (...args) => {
      const parent = resolveParent(argParent(args))
      const name = await api.input.prompt('File name:', 'untitled.md')
      if (!name) return
      const trimmed = name.trim()
      if (!trimmed) return
      // Default extension to `.md` when the user typed a bare name —
      // matches the toolbar "New note" button.
      const withExt = /\.[^/\\]+$/.test(trimmed) ? trimmed : `${trimmed}.md`
      const relpath = parent ? `${parent}/${withExt}` : withExt
      try {
        await createFile(relpath)
        useFilesStore.getState().setExpanded(parent, true)
        await refreshDir(parent)
        useFilesStore.getState().setSelected(relpath)
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Failed to create "${withExt}": ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })

    api.commands.register(COMMAND_CREATE_FOLDER, async (...args) => {
      const parent = resolveParent(argParent(args))
      const name = await api.input.prompt('Folder name:')
      if (!name) return
      const trimmed = name.trim()
      if (!trimmed) return
      const relpath = parent ? `${parent}/${trimmed}` : trimmed
      try {
        await createDir(relpath)
        useFilesStore.getState().setExpanded(parent, true)
        useFilesStore.getState().setExpanded(relpath, true)
        await refreshDir(parent)
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Failed to create "${trimmed}": ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })

    api.commands.register(COMMAND_RENAME, async (...args) => {
      const target = argRelpath(args) ?? useFilesStore.getState().selected
      if (!target) return
      const currentName = basename(target)
      const next = await api.input.prompt('Rename to:', currentName)
      if (!next) return
      const trimmed = next.trim()
      if (!trimmed || trimmed === currentName) return
      const parent = parentRel(target)
      const dst = parent ? `${parent}/${trimmed}` : trimmed
      try {
        await renameEntry(target, dst)
        useFilesStore.getState().setSelected(dst)
        // The watcher's file_renamed event refreshes the parent
        // listing, but call it eagerly to avoid a perceptible lag.
        await refreshDir(parent)
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Failed to rename: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })

    api.commands.register(COMMAND_DELETE, async (...args) => {
      const target = argRelpath(args) ?? useFilesStore.getState().selected
      if (!target) return
      const entry = findEntry(target)
      const isDir = entry?.isDir ?? false
      const name = basename(target)
      const ok = await api.input.confirm(
        isDir
          ? `Delete folder "${name}" and everything inside? This cannot be undone.`
          : `Delete "${name}"? This cannot be undone.`,
      )
      if (!ok) return
      try {
        await deleteEntry(target)
        // Clear selection if we just deleted the selected node.
        if (useFilesStore.getState().selected === target) {
          useFilesStore.getState().setSelected(null)
        }
        await refreshDir(parentRel(target))
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Failed to delete: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })

    /** Best-effort absolute-path join. Mirrors editor/index.ts —
     *  picks `\` only when the root is clearly a Windows path. */
    const joinAbsPath = (root: string, rel: string): string => {
      const sep = root.includes('\\') && !root.includes('/') ? '\\' : '/'
      const trimmed = root.endsWith('/') || root.endsWith('\\') ? root.slice(0, -1) : root
      return rel ? `${trimmed}${sep}${rel}` : trimmed
    }

    const parentDirOfAbs = (path: string): string => {
      const idx = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
      if (idx <= 0) return path
      return path.slice(0, idx)
    }

    api.commands.register(COMMAND_REVEAL, async (...args) => {
      const target = argRelpath(args) ?? useFilesStore.getState().selected
      const root = useWorkspaceStore.getState().rootPath
      if (!root) {
        api.notifications.show({ type: 'warning', message: 'No workspace open.' })
        return
      }
      // Mirror editor's COMMAND_REVEAL_IN_OS: we open the *parent*
      // directory in the OS file manager. For a directory target we
      // open the directory itself (the legacy app's behavior was
      // equivalent — `openExternal` on a dir opens the file manager
      // there).
      const abs = target ? joinAbsPath(root, target) : root
      const entry = target ? findEntry(target) : null
      const reveal = !target || (entry?.isDir ?? false) ? abs : parentDirOfAbs(abs)
      try {
        await api.platform.shell.openExternal(reveal)
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Reveal failed: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })

    api.commands.register(COMMAND_COPY_PATH, async (...args) => {
      const target = argRelpath(args) ?? useFilesStore.getState().selected
      if (!target) return
      // Match the legacy file tree: copy the forge-relative path. The
      // editor plugin offers separate "Copy Path (absolute)" if users
      // want the abs form.
      try {
        await navigator.clipboard.writeText(target)
        api.notifications.show({ type: 'info', message: 'Copied path.' })
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Copy failed: ${err instanceof Error ? err.message : String(err)}`,
        })
      }
    })

    // ── Live refresh on storage events ───────────────────────────────────
    //
    // The storage plugin's bridge thread (see
    // crates/nexus-storage/src/core_plugin.rs::publish_event) translates
    // every filesystem change its watcher observes into a
    // `com.nexus.storage.file_{created,modified,deleted,renamed}` custom
    // event on the kernel bus. Each payload carries a forge-relative
    // path (named `path` for create/modify/delete, `from`+`to` for
    // rename). We invalidate the parent directory's cached listing and
    // re-fetch it only when that parent is currently in the cache —
    // directories the user never expanded stay cold.
    //
    // Subscription cleanup: `api.kernel.on` resolves with an unsubscribe
    // fn. nexus.files has no `deactivate` (runs once at startup, never
    // torn down in normal operation). We explicitly drop the previous
    // subscriptions on each `workspace:closed` so a kernel shutdown
    // doesn't leave dangling forwarder tasks, and re-subscribe on the
    // next `workspace:opened`. Any further leak-on-reload is acceptable:
    // HMR / plugin disable isn't a supported user workflow here, and
    // the parent bridge task is torn down on window close via
    // `WindowEvent::CloseRequested`.

    const refreshParent = (parent: string) => {
      const cached = useFilesStore.getState().children[parent]
      if (!cached) return
      loadChildren(parent).then((entries) => {
        useFilesStore.getState().setChildren(parent, entries)
      })
    }

    const handleFsEvent = (_topic: string, payload: unknown) => {
      const paths = payloadPaths(payload)
      if (paths.length === 0) return
      const parents = new Set(paths.map(parentRelpath))
      for (const parent of parents) refreshParent(parent)
      // BL-053 Phase 4 — invalidate the per-path status cache so
      // the file-tree dot picks up frontmatter changes the next
      // time the row asks for it.
      const store = useStatusStore.getState()
      for (const path of paths) store.invalidate(path)
    }

    let fsUnsubs: Array<() => void> = []

    const subscribeFsEvents = async () => {
      if (fsUnsubs.length > 0) return // already subscribed
      try {
        fsUnsubs = await Promise.all([
          api.kernel.on(TOPIC_FILE_CREATED, handleFsEvent),
          api.kernel.on(TOPIC_FILE_MODIFIED, handleFsEvent),
          api.kernel.on(TOPIC_FILE_DELETED, handleFsEvent),
          api.kernel.on(TOPIC_FILE_RENAMED, handleFsEvent),
        ])
      } catch (err) {
        clientLogger.warn('[nexus.files] failed to subscribe to storage events:', err)
        fsUnsubs = []
      }
    }

    const unsubscribeFsEvents = () => {
      for (const unsub of fsUnsubs) {
        try {
          unsub()
        } catch (err) {
          clientLogger.warn('[nexus.files] unsubscribe failed:', err)
        }
      }
      fsUnsubs = []
    }

    // Reset the tree cache when the workspace closes so stale entries
    // don't show after the user points Nexus at a different folder.
    // Pair each transition with the kernel subscription lifecycle.
    //
    // Reading `on_init` in the storage plugin: it only opens the
    // `StorageEngine`; `on_start` spawns the watcher thread.
    // `notify-debouncer-mini` does NOT replay existing-file events on
    // start — it only fires on real disk changes — so no bootstrap
    // flood of `file_created` events is expected. No debounce /
    // coalescing is added here. If that turns out to be wrong in
    // practice, the fix is a per-parent 200ms trailing debounce inside
    // `refreshParent`.
    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      useFilesStore.getState().reset()
      void subscribeFsEvents()
    })
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      useFilesStore.getState().reset()
      unsubscribeFsEvents()
    })

    // Workspace restoration happens synchronously inside
    // nexus.workspace.activate (see shell/src/plugins/nexus/workspace/
    // index.ts) and emits `workspace:opened` BEFORE this plugin's
    // listener is registered on first boot. Cover that race by
    // subscribing immediately if the kernel is already up.
    if (await api.kernel.available()) {
      void subscribeFsEvents()
    }
  },
}
