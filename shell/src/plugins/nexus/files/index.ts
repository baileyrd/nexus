import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { FilesTree } from './FilesTree'
import { useFilesStore, type FilesDirEntry } from './filesStore'
import { loadChildren, setKernel } from './kernelClient'

const VIEW_ID = 'nexus.files.tree'
const EVENT_FILE_OPEN = 'files:open'
const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
// Topic strings verified against crates/nexus-storage/src/core_plugin.rs::publish_event.
const TOPIC_FILE_CREATED = 'com.nexus.storage.file_created'
const TOPIC_FILE_MODIFIED = 'com.nexus.storage.file_modified'
const TOPIC_FILE_DELETED = 'com.nexus.storage.file_deleted'
const TOPIC_FILE_RENAMED = 'com.nexus.storage.file_renamed'

// Lucide-style folder path. Stroke-only, 24×24 viewbox.
const FOLDER_ICON_PATH =
  'M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.93a2 2 0 0 1-1.66-.9l-.82-1.2A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13c0 1.1.9 2 2 2z'

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
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'nexus.sidebar'],
    contributes: {},
  },

  async activate(api: PluginAPI) {
    setKernel(api.kernel)

    const handleFileActivate = (entry: FilesDirEntry) => {
      api.events.emit(EVENT_FILE_OPEN, {
        relpath: entry.relpath,
        name: entry.name,
      })
    }

    api.views.register(VIEW_ID, {
      slot: 'sidebarContent',
      component: () => createElement(FilesTree, { onFileActivate: handleFileActivate }),
      priority: 10,
    })

    api.activityBar.addItem({
      id: 'nexus.files.activityItem',
      icon: '',
      iconPath: FOLDER_ICON_PATH,
      title: 'Files',
      viewId: VIEW_ID,
      priority: 10,
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
        console.warn('[nexus.files] failed to subscribe to storage events:', err)
        fsUnsubs = []
      }
    }

    const unsubscribeFsEvents = () => {
      for (const unsub of fsUnsubs) {
        try {
          unsub()
        } catch (err) {
          console.warn('[nexus.files] unsubscribe failed:', err)
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
