// Phase 5 workspace-View wrapper for the sidebar search panel
// (leaf-migration-plan.md §Phase 5). `SearchView` is the name the
// React component already uses; this file is named SearchPaneView to
// avoid a collision with that export.

import { createRoot, type Root } from 'react-dom/client'
import type { ReactElement } from 'react'
import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'

type RenderFn = () => ReactElement

/** Wraps `<SearchView>` for leaf mounting. State lives in `useSearchStore`. */
export class SearchPaneView extends ViewBase {
  readonly viewType = 'search'
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

export function searchPaneViewCreator(render: RenderFn): ViewCreator {
  return (leaf) => new SearchPaneView(leaf, render)
}
