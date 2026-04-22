import { createRoot, type Root } from 'react-dom/client'
import type { ReactElement } from 'react'
import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'

type RenderFn = () => ReactElement

export class GraphGlobalPaneView extends ViewBase {
  // Distinct from the local-neighbourhood sidecar's `'graph'` viewType.
  // Renaming the sidecar's key would silently morph existing
  // workspace.json leaves into the global view, so we use a fresh key
  // here and let the sidecar keep the historical id.
  readonly viewType = 'graph-global'
  private root: Root | null = null
  private readonly render: RenderFn

  constructor(leaf: Leaf, render: RenderFn) {
    super(leaf)
    this.render = render
  }

  getDisplayText(): string {
    return 'Graph'
  }

  getIcon(): string {
    return 'graph'
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

export function graphGlobalPaneViewCreator(render: RenderFn): ViewCreator {
  return (leaf) => new GraphGlobalPaneView(leaf, render)
}
