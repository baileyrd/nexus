// BL-054 Phase 4 — leaf wrapper for the observability panel.

import { createRoot, type Root } from 'react-dom/client'
import type { ReactElement } from 'react'
import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'

type RenderFn = () => ReactElement

export class OsObservabilityPaneView extends ViewBase {
  readonly viewType = 'osObservability'
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

export function osObservabilityPaneViewCreator(render: RenderFn): ViewCreator {
  return (leaf) => new OsObservabilityPaneView(leaf, render)
}
