// shell/src/plugins/nexus/searchPanel/index.tsx
//
// BL-078 — multi-file find/replace panel.
//
// Sidebar leaf, sibling to Saved Commands / History / Cross-Search.
// Triggered by ⌘⇧F (the VS Code "find in files" muscle memory) or
// from the command palette. Wraps `com.nexus.storage::find_in_files`
// + `replace_in_files` (BL-078 backend handlers).

import { createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { ViewBase, workspace, type Leaf } from '../../../workspace'
import { SearchPanelView } from './SearchPanelView'
import { useSearchPanelStore } from './searchPanelStore'
import './searchPanel.css'

const VIEW_TYPE = 'search-panel'
const COMMAND_FOCUS = 'nexus.searchPanel.focus'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

class SearchPaneView extends ViewBase {
  readonly viewType = VIEW_TYPE
  private root: Root | null = null
  private readonly render: () => React.ReactElement

  constructor(leaf: Leaf, render: () => React.ReactElement) {
    super(leaf)
    this.render = render
  }

  onOpen(el: HTMLElement): void {
    this.root = createRoot(el)
    this.root.render(this.render())
  }

  onClose(): void {
    this.root?.unmount()
    this.root = null
  }
}

export const searchPanelPlugin: Plugin = {
  manifest: {
    id: 'nexus.searchPanel',
    name: 'Search in Files',
    version: '0.1.0',
    core: false,
    // Lazy activation — only when the user invokes the command or
    // the workspace hydrates a leaf of this type.
    activationEvents: [`onCommand:${COMMAND_FOCUS}`, `onView:${VIEW_TYPE}`],
    dependsOn: ['com.nexus.storage'],
    contributes: {
      commands: [
        {
          id: COMMAND_FOCUS,
          title: 'Search in Files',
          category: 'Search',
        },
      ],
      keybindings: [
        // VS Code / Sublime convention: ⌘⇧F opens "find in files"
        // workspace-wide. Terminal cross-search (BL-063) moved to
        // ⌘⇧G to free this binding for the workspace surface.
        {
          command: COMMAND_FOCUS,
          key: 'ctrl+shift+f',
          mac: 'cmd+shift+f',
        },
      ],
    },
  },

  activate(api: PluginAPI) {
    api.viewRegistry.register(VIEW_TYPE, (leaf) =>
      new SearchPaneView(leaf, () =>
        createElement(SearchPanelView, {
          kernel: api.kernel,
          events: api.events,
        }),
      ),
    )

    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'left')
      workspace.revealLeaf(leaf)
    })

    // Fresh workspace → fresh state. Otherwise an old query / result
    // set from the previous forge would linger across opens.
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      useSearchPanelStore.getState().reset()
    })
  },
}
