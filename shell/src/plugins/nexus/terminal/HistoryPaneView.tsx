// shell/src/plugins/nexus/terminal/HistoryPaneView.tsx
//
// BL-060 — Leaf wrapper for HistoryView. Mirrors the
// SavedCommandsPaneView pattern: the React tree owns its DOM via
// createRoot; the ViewBase contract just brackets mount/unmount.

import { createRoot, type Root } from 'react-dom/client'
import type { ReactElement } from 'react'
import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'

export const HISTORY_VIEW_TYPE = 'terminal-history'

type RenderFn = () => ReactElement

export class HistoryPaneView extends ViewBase {
  readonly viewType = HISTORY_VIEW_TYPE
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

export function historyPaneViewCreator(render: RenderFn): ViewCreator {
  return (leaf) => new HistoryPaneView(leaf, render)
}
