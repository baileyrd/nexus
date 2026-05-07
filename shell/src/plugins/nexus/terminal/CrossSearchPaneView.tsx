// shell/src/plugins/nexus/terminal/CrossSearchPaneView.tsx
//
// BL-063 — Leaf wrapper for CrossSearchView. Mirrors the
// SavedCommandsPaneView / HistoryPaneView pattern.

import { createRoot, type Root } from 'react-dom/client'
import type { ReactElement } from 'react'
import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'

export const CROSS_SEARCH_VIEW_TYPE = 'terminal-cross-search'

type RenderFn = () => ReactElement

export class CrossSearchPaneView extends ViewBase {
  readonly viewType = CROSS_SEARCH_VIEW_TYPE
  private root: Root | null = null
  private readonly render: RenderFn

  constructor(leaf: Leaf, render: RenderFn) {
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

export function crossSearchPaneViewCreator(render: RenderFn): ViewCreator {
  return (leaf) => new CrossSearchPaneView(leaf, render)
}
