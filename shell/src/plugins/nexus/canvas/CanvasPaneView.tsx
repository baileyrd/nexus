// Workspace View wrapper for .canvas files. Mounts a React root that
// renders CanvasView with the leaf's relpath. Mirrors MarkdownView /
// TerminalPaneView in structure — state is just `{ relpath }` so
// workspace layout serialization round-trips cleanly.

import { createRoot, type Root } from 'react-dom/client'
import type { ReactElement } from 'react'
import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'

interface CanvasViewState {
  relpath?: string
}

type RenderFn = (relpath: string | undefined) => ReactElement

export class CanvasPaneView extends ViewBase {
  readonly viewType = 'canvas'
  private root: Root | null = null
  private state: CanvasViewState = {}
  private readonly render: RenderFn

  constructor(leaf: Leaf, render: RenderFn) {
    super(leaf)
    this.render = render
  }

  getState(): CanvasViewState {
    return this.state
  }

  getDisplayText(): string {
    const relpath = this.state.relpath
    if (!relpath) return this.viewType
    const i = Math.max(relpath.lastIndexOf('/'), relpath.lastIndexOf('\\'))
    return i >= 0 ? relpath.slice(i + 1) : relpath
  }

  setState(state: unknown): void {
    if (state && typeof state === 'object' && 'relpath' in state) {
      const relpath = (state as Record<string, unknown>).relpath
      this.state = { relpath: typeof relpath === 'string' ? relpath : undefined }
    } else {
      this.state = {}
    }
  }

  onOpen(el: HTMLElement): void {
    this.root = createRoot(el)
    this.root.render(this.render(this.state.relpath))
  }

  onClose(): void {
    this.root?.unmount()
    this.root = null
  }
}

export function canvasPaneViewCreator(render: RenderFn): ViewCreator {
  return (leaf) => new CanvasPaneView(leaf, render)
}
