// Workspace-View wrapper for the Note Context accordion panel.
// Mirrors `nexus.backlinks/BacklinkView.tsx` — ViewBase subclass that
// owns the React root for the panel's content area.

import { createRoot, type Root } from 'react-dom/client'
import { createElement } from 'react'
import type { Leaf } from '../../../workspace'
import { ViewBase } from '../../../workspace'
import { NoteContextView } from './NoteContextView'

export class NoteContextPaneView extends ViewBase {
  readonly viewType = 'note-context'
  private root: Root | null = null

  constructor(leaf: Leaf) {
    super(leaf)
  }

  onOpen(el: HTMLElement): void {
    this.root = createRoot(el)
    this.root.render(createElement(NoteContextView))
  }

  onClose(): void {
    this.root?.unmount()
    this.root = null
  }
}
