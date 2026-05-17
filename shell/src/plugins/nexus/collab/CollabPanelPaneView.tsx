import { createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'
import { CollabPanel } from './CollabPanel'

export class CollabPanelPaneView extends ViewBase {
  readonly viewType = 'collab-panel'
  private root: Root | null = null
  private rootedEl: HTMLElement | null = null

  constructor(leaf: Leaf) {
    super(leaf)
  }

  onOpen(el: HTMLElement): void {
    if (this.root && this.rootedEl === el) {
      this.root.render(createElement(CollabPanel))
      return
    }
    this.root = createRoot(el)
    this.rootedEl = el
    this.root.render(createElement(CollabPanel))
  }

  onClose(): void {
    const root = this.root
    this.root = null
    this.rootedEl = null
    if (root) queueMicrotask(() => root.unmount())
  }
}

export function collabPanelViewCreator(): ViewCreator {
  return (leaf) => new CollabPanelPaneView(leaf)
}
