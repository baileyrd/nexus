// Workspace-View wrapper for the Templates listing. Mirrors
// SkillsPaneView; render fn is provided by the plugin's index.ts so
// store wiring stays in the plugin module.

import { createRoot, type Root } from 'react-dom/client'
import type { ReactElement } from 'react'
import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'

type RenderFn = () => ReactElement

export class TemplatesPaneView extends ViewBase {
  readonly viewType = 'templates'
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

export function templatesPaneViewCreator(render: RenderFn): ViewCreator {
  return (leaf) => new TemplatesPaneView(leaf, render)
}
