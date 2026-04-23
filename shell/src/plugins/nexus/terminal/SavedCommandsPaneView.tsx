// shell/src/plugins/nexus/terminal/SavedCommandsPaneView.tsx
//
// WI-05 — Leaf wrapper for SavedCommandsView. Mirrors the
// TerminalPaneView pattern (Phase 5 Leaf/View shape): the React tree
// owns its DOM via createRoot; the ViewBase contract just brackets the
// mount/unmount.

import { createRoot, type Root } from 'react-dom/client'
import type { ReactElement } from 'react'
import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'

export const SAVED_COMMANDS_VIEW_TYPE = 'terminal-saved-commands'

type RenderFn = () => ReactElement

export class SavedCommandsPaneView extends ViewBase {
  readonly viewType = SAVED_COMMANDS_VIEW_TYPE
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

export function savedCommandsPaneViewCreator(render: RenderFn): ViewCreator {
  return (leaf) => new SavedCommandsPaneView(leaf, render)
}
