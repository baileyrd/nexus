// Workspace View wrapper for `.bases` directories. Mounts a React
// root that renders BasesView with the leaf's relpath. Mirrors
// CanvasPaneView — state is just `{ relpath }` so workspace layout
// serialization round-trips.

import { createRoot, type Root } from 'react-dom/client'
import type { ReactElement } from 'react'
import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'
import { useBasesStore } from './basesStore'

interface BasesViewState {
  relpath?: string
}

type RenderFn = (relpath: string | undefined) => ReactElement

export class BasesPaneView extends ViewBase {
  readonly viewType = 'bases'
  private root: Root | null = null
  private state: BasesViewState = {}
  private readonly render: RenderFn

  constructor(leaf: Leaf, render: RenderFn) {
    super(leaf)
    this.render = render
  }

  getState(): BasesViewState {
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
    if (this.state.relpath) {
      useBasesStore.getState().closeTab(this.state.relpath)
    }
  }
}

export function basesPaneViewCreator(render: RenderFn): ViewCreator {
  return (leaf) => new BasesPaneView(leaf, render)
}
