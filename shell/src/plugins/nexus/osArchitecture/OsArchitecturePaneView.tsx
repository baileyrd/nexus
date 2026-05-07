// BL-054 Phase 2 — leaf wrapper for the architecture panel. Mirrors
// SkillsPaneView / WorkflowPaneView (leaf-migration-plan.md §Phase 5).

import { createRoot, type Root } from 'react-dom/client'
import type { ReactElement } from 'react'
import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'

type RenderFn = () => ReactElement

export class OsArchitecturePaneView extends ViewBase {
  readonly viewType = 'osArchitecture'
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

export function osArchitecturePaneViewCreator(render: RenderFn): ViewCreator {
  return (leaf) => new OsArchitecturePaneView(leaf, render)
}
