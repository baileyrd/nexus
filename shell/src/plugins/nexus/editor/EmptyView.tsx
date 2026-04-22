import { createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import type { Leaf, View, ViewCreator, ViewState } from '../../../workspace'
import { useEditorStore } from './editorStore'
import { EmptyStateActions } from './EditorView'

/**
 * Empty-view creator used for leaves that have no file (fresh tabs
 * spawned by the `+` new-tab button, or the initial workspace
 * placeholder). Mounts the same EmptyStateActions component the
 * EditorView renders on its !activeTab branch so empty leaves get
 * the Obsidian-style action links (Create new note / Go to file /
 * Close) instead of a blank pane.
 *
 * Overrides the default no-op empty view registered in
 * shell/src/workspace/ViewRegistry.ts. The default is still used as
 * a fallback for leaves registered with no type at all.
 */
export const EMPTY_VIEW_TYPE = 'empty'

class EmptyView implements View {
  readonly viewType = EMPTY_VIEW_TYPE
  leaf: Leaf
  private root: Root | null = null
  private hostEl: HTMLDivElement | null = null

  constructor(leaf: Leaf) {
    this.leaf = leaf
  }

  getState(): ViewState['state'] {
    return {}
  }

  setState(): void {
    // No persisted state for empty views.
  }

  onOpen(el: HTMLElement): void {
    // Render into a dedicated child div so we never mutate the
    // LeafHost container's inline style — the host manages
    // display:none for inactive leaves, and writing to its .style
    // directly races React's style prop and can leave inactive
    // leaves visible on top of the active one.
    const host = document.createElement('div')
    host.className = 'empty-view-host'
    host.style.display = 'flex'
    host.style.flexDirection = 'column'
    host.style.alignItems = 'center'
    host.style.justifyContent = 'center'
    host.style.width = '100%'
    host.style.height = '100%'
    host.style.color = 'var(--fg-dim, var(--fg-muted, #888))'
    el.appendChild(host)
    this.hostEl = host

    this.root = createRoot(host)
    const hasAnyTab = useEditorStore.getState().tabs.length > 0
    this.root.render(createElement(EmptyStateActions, { hasAnyTab }))
  }

  onClose(): void {
    this.root?.unmount()
    this.root = null
    if (this.hostEl && this.hostEl.parentNode) {
      this.hostEl.parentNode.removeChild(this.hostEl)
    }
    this.hostEl = null
  }

  getDisplayText(): string {
    return 'New tab'
  }
}

export const emptyViewCreator: ViewCreator = (leaf) => new EmptyView(leaf)
