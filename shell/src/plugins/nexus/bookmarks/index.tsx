import { createRoot, type Root } from 'react-dom/client'
import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { ViewBase, viewRegistry, workspace, type Leaf } from '../../../workspace'

const VIEW_TYPE = 'bookmarks'
const COMMAND_FOCUS = 'nexus.bookmarks.focus'

function BookmarksView() {
  return (
    <div
      style={{
        padding: 16,
        fontSize: 12,
        color: 'var(--fg-dim)',
        lineHeight: 1.5,
      }}
    >
      Not yet implemented. This inspector will list saved bookmarks
      grouped by collection.
    </div>
  )
}

/**
 * Phase 7: bookmarks now mounts as a Leaf-hosted View. Placeholder
 * component will be replaced when real bookmark storage ships.
 */
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
    // WI-19 — lazy activation. Reached only via the focus command or
    // a hydrated leaf of this view type; both fire the matching
    // trigger before activate() is needed.
    activationEvents: [`onCommand:${COMMAND_FOCUS}`, `onView:${VIEW_TYPE}`],
    contributes: {
      commands: [{ id: COMMAND_FOCUS, title: 'Focus Bookmarks', category: 'View' }],
    },
  },

  activate(api: PluginAPI) {
    viewRegistry.register(VIEW_TYPE, (leaf) => new BookmarksPaneView(leaf))

    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'right')
      workspace.revealLeaf(leaf)
    })
  },
}
