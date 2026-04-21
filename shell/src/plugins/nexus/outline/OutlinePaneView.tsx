// Phase 5 workspace-View wrapper for the Outline inspector
// (leaf-migration-plan.md §Phase 5). State is owned by
// `useOutlineStore`; this View is a thin React-mount shim.

import { createRoot, type Root } from 'react-dom/client'
import type { ReactElement } from 'react'
import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'

type RenderFn = () => ReactElement

export class OutlinePaneView extends ViewBase {
  readonly viewType = 'outline'
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

export function outlinePaneViewCreator(render: RenderFn): ViewCreator {
  return (leaf) => new OutlinePaneView(leaf, render)
}
