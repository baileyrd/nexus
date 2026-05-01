import { createRoot, type Root } from 'react-dom/client'
import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { ViewBase, viewRegistry, workspace, type Leaf } from '../../../workspace'

const VIEW_TYPE = 'tags'
const COMMAND_FOCUS = 'nexus.tags.focus'

/** Placeholder body. A tag inspector — listing the active note's
 *  tags and offering filters into the global tag index — is not yet
 *  implemented. Tab + command exist so the titlebar shortcut resolves. */
function TagsView() {
  return (
    <div
      style={{
        padding: 16,
        fontSize: 12,
        color: 'var(--text-faint)',
        lineHeight: 1.5,
      }}
    >
      Not yet implemented. This inspector will surface the active
      note's tags and their usage across the workspace.
    </div>
  )
}

class TagsPaneView extends ViewBase {
  readonly viewType = VIEW_TYPE
  private root: Root | null = null

  constructor(leaf: Leaf) {
    super(leaf)
  }

  async onOpen(containerEl: HTMLElement): Promise<void> {
    this.root = createRoot(containerEl)
    this.root.render(createElement(TagsView))
  }

  async onClose(): Promise<void> {
    this.root?.unmount()
    this.root = null
  }
}

export const tagsPlugin: Plugin = {
  manifest: {
    id: 'nexus.tags',
    name: 'Tags',
    version: '0.1.0',
    core: false,
    // WI-19 — lazy activation. Reached only via the focus command or
    // a hydrated leaf of this view type; both fire the matching
    // trigger before activate() is needed.
    activationEvents: [`onCommand:${COMMAND_FOCUS}`, `onView:${VIEW_TYPE}`],
    contributes: {
      commands: [{ id: COMMAND_FOCUS, title: 'Focus Tags', category: 'View' }],
    },
  },

  activate(api: PluginAPI) {
    viewRegistry.register(VIEW_TYPE, (leaf) => new TagsPaneView(leaf))
    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'right')
      workspace.revealLeaf(leaf)
    })
  },
}
