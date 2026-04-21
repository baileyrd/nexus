// Phase 5 workspace-View wrapper for the neighbour-graph inspector
// (leaf-migration-plan.md §Phase 5). State lives in `useGraphStore`.

import { createRoot, type Root } from 'react-dom/client'
import type { ReactElement } from 'react'
import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'

type RenderFn = () => ReactElement

export class GraphPaneView extends ViewBase {
  readonly viewType = 'graph'
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

export function graphPaneViewCreator(render: RenderFn): ViewCreator {
  return (leaf) => new GraphPaneView(leaf, render)
}
