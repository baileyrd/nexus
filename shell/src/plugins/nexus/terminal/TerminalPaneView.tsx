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
  private rootedEl: HTMLElement | null = null
  private readonly render: RenderFn

  constructor(leaf: Leaf, render: RenderFn) {
    super(leaf)
    this.render = render
  }

  onOpen(el: HTMLElement): void {
    // React 18 StrictMode dev fires effect mounts twice on first
    // attach. The second `attachContainer` arrives with the same
    // `el` while our first root is still live; calling
    // `createRoot(el)` again would trip "container has already been
    // passed to createRoot before". Treat same-`el` as a re-render.
    if (this.root && this.rootedEl === el) {
      this.root.render(this.render())
      return
    }
    this.root = createRoot(el)
    this.rootedEl = el
    this.root.render(this.render())
  }

  onClose(): void {
    const root = this.root
    this.root = null
    this.rootedEl = null
    if (root) {
      // Defer so React's current commit finishes before unmount lands.
      // Without this, `Leaf.attachContainer`'s synchronous close+open
      // re-home trips "Attempted to synchronously unmount a root
      // while React was already rendering."
      queueMicrotask(() => root.unmount())
    }
  }
}

export function terminalPaneViewCreator(render: RenderFn): ViewCreator {
  return (leaf) => new TerminalPaneView(leaf, render)
}
