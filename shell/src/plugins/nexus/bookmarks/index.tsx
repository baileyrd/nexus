import { createRoot, type Root } from 'react-dom/client'
import { createElement, useEffect, useState } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { ViewBase, workspace, type Leaf } from '../../../workspace'
import { useEditorStore } from '../editor/editorStore'
import { useConfigStore } from '../../../stores/configStore'
import type { EventsAPI } from '../../../types/plugin'

let events: EventsAPI | null = null

const VIEW_TYPE = 'bookmarks'
const COMMAND_FOCUS = 'nexus.bookmarks.focus'
const COMMAND_TOGGLE = 'nexus.bookmarks.toggleActive'
const SETTING_KEY = 'nexus.bookmarks.entries'

interface BookmarkEntry {
  relpath: string
  label: string
  createdAt: number
}

function basename(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? relpath : relpath.slice(i + 1)
}

function readBookmarks(): BookmarkEntry[] {
  const raw = useConfigStore.getState().get<unknown>(SETTING_KEY, [])
  if (!Array.isArray(raw)) return []
  const out: BookmarkEntry[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const r = item as Record<string, unknown>
    if (typeof r.relpath !== 'string' || r.relpath.length === 0) continue
    out.push({
      relpath: r.relpath,
      label: typeof r.label === 'string' && r.label.length > 0 ? r.label : basename(r.relpath),
      createdAt: typeof r.createdAt === 'number' ? r.createdAt : 0,
    })
  }
  return out
}

function writeBookmarks(entries: BookmarkEntry[]): void {
  useConfigStore.getState().set(SETTING_KEY, entries)
}

function toggleBookmark(relpath: string): void {
  const current = readBookmarks()
  const exists = current.some((b) => b.relpath === relpath)
  if (exists) {
    writeBookmarks(current.filter((b) => b.relpath !== relpath))
  } else {
    writeBookmarks([
      ...current,
      { relpath, label: basename(relpath), createdAt: Date.now() },
    ])
  }
}

function BookmarksView() {
  const activeRelpath = useEditorStore((s) => s.activeRelpath)
  const [entries, setEntries] = useState<BookmarkEntry[]>(() => readBookmarks())

  useEffect(() => {
    const reload = () => setEntries(readBookmarks())
    const unsub = events?.on(`config:changed:${SETTING_KEY}`, reload) ?? (() => {})
    // Re-read once after mount in case the configStore hydrated after
    // the initial useState evaluation.
    reload()
    return () => {
      unsub()
    }
  }, [])

  const activeIsBookmarked =
    activeRelpath !== null && entries.some((b) => b.relpath === activeRelpath)

  return (
    <div style={{ padding: 8, fontSize: 13, display: 'flex', flexDirection: 'column', gap: 4 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', padding: '4px 8px' }}>
        <span style={{ color: 'var(--text-muted)', fontSize: 11 }}>
          {entries.length} {entries.length === 1 ? 'bookmark' : 'bookmarks'}
        </span>
        {activeRelpath && (
          <button
            type="button"
            onClick={() => toggleBookmark(activeRelpath)}
            style={{
              fontSize: 11,
              padding: '2px 8px',
              cursor: 'pointer',
              background: 'transparent',
              color: 'var(--text-normal)',
              border: '1px solid var(--background-modifier-border)',
              borderRadius: 4,
            }}
          >
            {activeIsBookmarked ? 'Remove bookmark' : 'Bookmark active'}
          </button>
        )}
      </div>
      {entries.length === 0 ? (
        <div style={{ padding: 16, fontSize: 12, color: 'var(--text-faint)' }}>
          No bookmarks yet.
        </div>
      ) : (
        entries.map((b) => (
          <div
            key={b.relpath}
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              padding: '4px 8px',
              color: 'var(--text-normal)',
            }}
          >
            <span
              onClick={() => events?.emit('files:open', { relpath: b.relpath, name: b.label })}
              style={{ cursor: 'pointer', flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}
              title={b.relpath}
            >
              {b.label}
            </span>
            <button
              type="button"
              onClick={() => toggleBookmark(b.relpath)}
              title="Remove bookmark"
              style={{
                marginLeft: 8,
                background: 'transparent',
                color: 'var(--text-faint)',
                border: 'none',
                cursor: 'pointer',
                fontSize: 12,
              }}
            >
              ×
            </button>
          </div>
        ))
      )}
    </div>
  )
}

class BookmarksPaneView extends ViewBase {
  readonly viewType = VIEW_TYPE
  private root: Root | null = null

  constructor(leaf: Leaf) {
    super(leaf)
  }

  async onOpen(containerEl: HTMLElement): Promise<void> {
    this.root = createRoot(containerEl)
    this.root.render(createElement(BookmarksView))
  }

  async onClose(): Promise<void> {
    this.root?.unmount()
    this.root = null
  }
}

export const bookmarksPlugin: Plugin = {
  manifest: {
    id: 'nexus.bookmarks',
    name: 'Bookmarks',
    version: '0.1.0',
    core: false,
    activationEvents: [
      `onCommand:${COMMAND_FOCUS}`,
      `onCommand:${COMMAND_TOGGLE}`,
      `onView:${VIEW_TYPE}`,
    ],
    contributes: {
      commands: [
        { id: COMMAND_FOCUS, title: 'Focus Bookmarks', category: 'View' },
        { id: COMMAND_TOGGLE, title: 'Toggle bookmark for active note', category: 'View' },
      ],
    },
  },

  activate(api: PluginAPI) {
    events = api.events
    api.viewRegistry.register(VIEW_TYPE, (leaf) => new BookmarksPaneView(leaf))

    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'right')
      workspace.revealLeaf(leaf)
    })

    api.commands.register(COMMAND_TOGGLE, async () => {
      const active = useEditorStore.getState().activeRelpath
      if (!active) return
      toggleBookmark(active)
    })
  },
}
