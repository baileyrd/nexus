// Phase 5 workspace-View wrapper for the Skills listing
// (leaf-migration-plan.md §Phase 5). State lives in `useSkillsStore`.

import { createRoot, type Root } from 'react-dom/client'
import type { ReactElement } from 'react'
import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'

type RenderFn = () => ReactElement

export class SkillsPaneView extends ViewBase {
  readonly viewType = 'skills'
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

export function skillsPaneViewCreator(render: RenderFn): ViewCreator {
  return (leaf) => new SkillsPaneView(leaf, render)
}
