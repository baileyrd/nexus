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
    // Match the editor's layout conventions — centred vertically,
    // muted accent colour. The host leaf container is a flex column;
    // this wrapper fills it and centres the action stack.
    el.style.display = 'flex'
    el.style.flexDirection = 'column'
    el.style.alignItems = 'center'
    el.style.justifyContent = 'center'
    el.style.height = '100%'
    el.style.color = 'var(--fg-dim, var(--fg-muted, #888))'

    this.root = createRoot(el)
    const hasAnyTab = useEditorStore.getState().tabs.length > 0
    this.root.render(createElement(EmptyStateActions, { hasAnyTab }))
  }

  onClose(): void {
    this.root?.unmount()
    this.root = null
  }

  getDisplayText(): string {
    return 'New tab'
  }
}

export const emptyViewCreator: ViewCreator = (leaf) => new EmptyView(leaf)
