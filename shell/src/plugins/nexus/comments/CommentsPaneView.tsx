// Workspace-View wrapper for the side-margin comments pane (BL-050
// Phase 2). Mirrors the OutlinePaneView / BacklinkView pattern — the
// View itself is a thin React-mount shim; state lives in the
// `useCommentsStore` zustand store managed by index.ts.

import { createRoot, type Root } from 'react-dom/client'
import type { ReactElement } from 'react'
import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'

type RenderFn = () => ReactElement

export class CommentsPaneView extends ViewBase {
  readonly viewType = 'comments'
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

export function commentsPaneViewCreator(render: RenderFn): ViewCreator {
  return (leaf) => new CommentsPaneView(leaf, render)
}
