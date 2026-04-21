// Phase 5 workspace-View wrapper for the integrated terminal
// (leaf-migration-plan.md §Phase 5). The xterm instance lives inside
// the React component already — it handles its own imperative DOM,
// so wrapping it in a createRoot is a plain passthrough.

import { createRoot, type Root } from 'react-dom/client'
import type { ReactElement } from 'react'
import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'

type RenderFn = () => ReactElement

export class TerminalPaneView extends ViewBase {
  readonly viewType = 'terminal'
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

export function terminalPaneViewCreator(render: RenderFn): ViewCreator {
  return (leaf) => new TerminalPaneView(leaf, render)
}
