// BL-067 Phase 1 — leaf wrapper for the View Builder panel.
//
// Mirrors `OsObservabilityPaneView` — a thin `ViewBase` subclass that
// mounts a React tree on `onOpen` and tears it down on `onClose`.

import { createRoot, type Root } from 'react-dom/client'
import type { ReactElement } from 'react'

import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'

type RenderFn = () => ReactElement

export class ViewBuilderPaneView extends ViewBase {
  readonly viewType = 'viewBuilder'
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

export function viewBuilderPaneViewCreator(render: RenderFn): ViewCreator {
  return (leaf) => new ViewBuilderPaneView(leaf, render)
}
