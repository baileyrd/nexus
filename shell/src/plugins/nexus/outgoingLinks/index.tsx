import { createRoot, type Root } from 'react-dom/client'
import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { ViewBase, viewRegistry, workspace, type Leaf } from '../../../workspace'

const VIEW_TYPE = 'outgoing-links'
const COMMAND_FOCUS = 'nexus.outgoingLinks.focus'

/** Placeholder body. Extraction of outgoing links from the active
 *  editor buffer is not yet implemented; the tab + command are
 *  scaffolded so the titlebar shortcut has a real target. */
function OutgoingLinksView() {
  return (
    <div
      style={{
        padding: 16,
        fontSize: 12,
        color: 'var(--fg-dim)',
        lineHeight: 1.5,
      }}
    >
      Not yet implemented. This inspector will list outgoing links
      from the active note once a forward-link extractor ships.
    </div>
  )
}

class OutgoingLinksPaneView extends ViewBase {
  readonly viewType = VIEW_TYPE
  private root: Root | null = null

  constructor(leaf: Leaf) {
    super(leaf)
  }

  async onOpen(containerEl: HTMLElement): Promise<void> {
    this.root = createRoot(containerEl)
    this.root.render(createElement(OutgoingLinksView))
  }

  async onClose(): Promise<void> {
    this.root?.unmount()
    this.root = null
  }
}

export const outgoingLinksPlugin: Plugin = {
  manifest: {
    id: 'nexus.outgoingLinks',
    name: 'Outgoing Links',
    version: '0.1.0',
    core: false,
    // WI-19 — lazy activation. Reached only via the focus command or
    // a hydrated leaf of this view type; both fire the matching
    // trigger before activate() is needed.
    activationEvents: [`onCommand:${COMMAND_FOCUS}`, `onView:${VIEW_TYPE}`],
    contributes: {
      commands: [{ id: COMMAND_FOCUS, title: 'Focus Outgoing Links', category: 'View' }],
    },
  },

  activate(api: PluginAPI) {
    viewRegistry.register(VIEW_TYPE, (leaf) => new OutgoingLinksPaneView(leaf))
    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'right')
      workspace.revealLeaf(leaf)
    })
  },
}
