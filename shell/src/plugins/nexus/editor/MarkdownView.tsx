// Phase 5 workspace-View wrapper for the Markdown editor
// (leaf-migration-plan.md §Phase 5). This is the centerpiece View —
// one leaf per open tab in the Phase 6 model.
//
// The existing `EditorView` React component reads the whole tab set
// from `useEditorStore`; for now we mount that component verbatim so
// Phase 5 is a no-behaviour-change wrap. A future refactor splits the
// tab-set into a per-leaf file-path plus a shared content cache, but
// that belongs in the Phase 6/7 cleanup when the shell flips to
// `<Workspace>` rendering.

import { createRoot, type Root } from 'react-dom/client'
import type { ReactElement } from 'react'
import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'

type RenderFn = () => ReactElement

/**
 * Per-leaf state for a future per-tab model — today the editor store
 * owns everything, so this is just a placeholder that round-trips the
 * relpath so `workspaceStore.serialize()` carries it forward.
 */
interface MarkdownViewState {
  relpath?: string
}

export class MarkdownView extends ViewBase {
  readonly viewType = 'markdown'
  private root: Root | null = null
  private state: MarkdownViewState = {}
  private readonly render: RenderFn

  constructor(leaf: Leaf, render: RenderFn) {
    super(leaf)
    this.render = render
  }

  getState(): MarkdownViewState {
    return this.state
  }

  setState(state: unknown): void {
    // The shape is validated shallowly — anything else gets dropped
    // so a malformed persisted layout doesn't crash hydrate.
    if (state && typeof state === 'object' && 'relpath' in state) {
      const relpath = (state as Record<string, unknown>).relpath
      this.state = { relpath: typeof relpath === 'string' ? relpath : undefined }
    } else {
      this.state = {}
    }
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

export function markdownViewCreator(render: RenderFn): ViewCreator {
  return (leaf) => new MarkdownView(leaf, render)
}
