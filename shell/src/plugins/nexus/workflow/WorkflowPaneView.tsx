// Phase 5 workspace-View wrapper for the Workflows listing
// (leaf-migration-plan.md §Phase 5). State lives in `useWorkflowStore`.

import { createRoot, type Root } from 'react-dom/client'
import type { ReactElement } from 'react'
import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'

type RenderFn = () => ReactElement

export class WorkflowPaneView extends ViewBase {
  readonly viewType = 'workflow'
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

export function workflowPaneViewCreator(render: RenderFn): ViewCreator {
  return (leaf) => new WorkflowPaneView(leaf, render)
}
