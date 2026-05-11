import { createRoot, type Root } from 'react-dom/client'
import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { ViewBase, workspace, type Leaf } from '../../../workspace'

const VIEW_TYPE = 'file-properties'
const COMMAND_FOCUS = 'nexus.fileProperties.focus'

/** Placeholder body. Properties extraction from the active note's
 *  YAML frontmatter (title, tags, created/updated, custom keys) is
 *  deferred — this scaffolds the tab + command so the right-dock
 *  default layout has a 6th icon that matches Obsidian. */
function FilePropertiesView() {
  return (
    <div
      style={{
        padding: 16,
        fontSize: 12,
        color: 'var(--text-faint)',
        lineHeight: 1.5,
      }}
    >
      Not yet implemented. This inspector will show the active note's
      frontmatter properties (title, tags, dates, and any custom
      YAML keys) once a properties extractor ships.
    </div>
  )
}

class FilePropertiesPaneView extends ViewBase {
  readonly viewType = VIEW_TYPE
  private root: Root | null = null

  constructor(leaf: Leaf) {
    super(leaf)
  }

  getIcon(): string {
    return 'info'
  }

  async onOpen(containerEl: HTMLElement): Promise<void> {
    this.root = createRoot(containerEl)
    this.root.render(createElement(FilePropertiesView))
  }

  async onClose(): Promise<void> {
    this.root?.unmount()
    this.root = null
  }
}

export const filePropertiesPlugin: Plugin = {
  manifest: {
    id: 'nexus.fileProperties',
    name: 'File Properties',
    version: '0.1.0',
    core: false,
    // WI-19 — lazy activation. Reached only via the focus command or
    // a hydrated leaf of this view type; both fire the matching
    // trigger before activate() is needed.
    activationEvents: [`onCommand:${COMMAND_FOCUS}`, `onView:${VIEW_TYPE}`],
    contributes: {
      commands: [{ id: COMMAND_FOCUS, title: 'Focus File Properties', category: 'View' }],
    },
  },

  activate(api: PluginAPI) {
    api.viewRegistry.register(VIEW_TYPE, (leaf) => new FilePropertiesPaneView(leaf))
    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'right')
      workspace.revealLeaf(leaf)
    })
  },
}
