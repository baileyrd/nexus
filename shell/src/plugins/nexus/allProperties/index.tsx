import { createRoot, type Root } from 'react-dom/client'
import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { ViewBase, workspace, type Leaf } from '../../../workspace'

const VIEW_TYPE = 'all-properties'
const COMMAND_FOCUS = 'nexus.allProperties.focus'

/** Placeholder body. A frontmatter-properties inspector — table of
 *  every key/value on the active note plus inherited values — is not
 *  yet implemented. Tab + command exist so the titlebar shortcut
 *  resolves. */
function AllPropertiesView() {
  return (
    <div
      style={{
        padding: 16,
        fontSize: 12,
        color: 'var(--text-faint)',
        lineHeight: 1.5,
      }}
    >
      Not yet implemented. This inspector will list every frontmatter
      property on the active note, including inherited values.
    </div>
  )
}

class AllPropertiesPaneView extends ViewBase {
  readonly viewType = VIEW_TYPE
  private root: Root | null = null

  constructor(leaf: Leaf) {
    super(leaf)
  }

  async onOpen(containerEl: HTMLElement): Promise<void> {
    this.root = createRoot(containerEl)
    this.root.render(createElement(AllPropertiesView))
  }

  async onClose(): Promise<void> {
    this.root?.unmount()
    this.root = null
  }
}

export const allPropertiesPlugin: Plugin = {
  manifest: {
    id: 'nexus.allProperties',
    name: 'All Properties',
    version: '0.1.0',
    core: false,
    // WI-19 — lazy activation. The view is only reached via either
    // (a) the focus command (palette/keybinding) or (b) the persisted
    // workspace re-hydrating a leaf of this type. Both paths fire the
    // matching trigger before we're needed. No activity-bar item, no
    // right-panel tab registration — nothing else has to run at boot.
    activationEvents: [`onCommand:${COMMAND_FOCUS}`, `onView:${VIEW_TYPE}`],
    contributes: {
      commands: [{ id: COMMAND_FOCUS, title: 'Focus All Properties', category: 'View' }],
    },
  },

  activate(api: PluginAPI) {
    api.viewRegistry.register(VIEW_TYPE, (leaf) => new AllPropertiesPaneView(leaf))
    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'right')
      workspace.revealLeaf(leaf)
    })
  },
}
