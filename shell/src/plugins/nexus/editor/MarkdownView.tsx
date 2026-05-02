// Phase 5 workspace-View wrapper for the Markdown editor
// (leaf-migration-plan.md §Phase 5). This is the centerpiece View —
// one leaf per open tab in the Phase 6 model.
//
// Phase 3 of `docs/editor-transaction-wiring-plan.md` adds per-leaf
// kernel-session lifecycle: on `onOpen` the leaf acquires a refcount
// against a `SessionManager`; on `onClose` it releases. Actual content
// hydration still flows through `index.ts`'s `files:open` handler — the
// View just keeps the session pinned for as long as a leaf renders it
// (so splits / layout-restore don't force a close+reopen).

import { createRoot, type Root } from 'react-dom/client'
import type { ReactElement } from 'react'
import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase } from '../../../workspace'
import type { SessionManager } from './sessionManager.ts'

type RenderFn = (relpath: string | undefined, leafId: string) => ReactElement

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
  private readonly sessionManager: SessionManager | null
  /** Relpath we called `acquire` for — tracked so `onClose`'s release
   *  matches even if `setState` fires after the initial acquire. */
  private acquiredRelpath: string | null = null

  constructor(
    leaf: Leaf,
    render: RenderFn,
    sessionManager: SessionManager | null,
  ) {
    super(leaf)
    this.render = render
    this.sessionManager = sessionManager
  }

  getState(): MarkdownViewState {
    return this.state
  }

  /** Tab label for the workspace header — shows the file's basename
   *  (last path segment). Falls back to `viewType` when no relpath is
   *  set yet (brief window between creator and the first setState). */
  getDisplayText(): string {
    const relpath = this.state.relpath
    if (!relpath) return this.viewType
    const i = Math.max(relpath.lastIndexOf('/'), relpath.lastIndexOf('\\'))
    return i >= 0 ? relpath.slice(i + 1) : relpath
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
    // The leaf's `state.relpath` is set by `setState` before `onOpen`
    // runs (both for fresh opens and layout restore), so the render fn
    // can rely on it here. If a later `setState` changes the relpath,
    // callers must drive a re-render themselves — today `state.relpath`
    // is write-once per leaf lifetime.
    this.root.render(this.render(this.state.relpath, this.leaf.id))

    // Pin the kernel session as long as this leaf is mounted. Untitled
    // / empty relpaths resolve to `null` in SessionManager and are a
    // no-op. Fire-and-forget: hydration is driven by `files:open` in
    // index.ts; the acquire here is purely about refcount lifetime.
    const relpath = this.state.relpath
    if (this.sessionManager && relpath) {
      this.acquiredRelpath = relpath
      void this.sessionManager.acquire(relpath)
    }
  }

  onClose(): void {
    this.root?.unmount()
    this.root = null

    if (this.sessionManager && this.acquiredRelpath) {
      const relpath = this.acquiredRelpath
      this.acquiredRelpath = null
      void this.sessionManager.release(relpath)
    }
  }
}

export function markdownViewCreator(
  render: RenderFn,
  sessionManager: SessionManager | null = null,
): ViewCreator {
  return (leaf) => new MarkdownView(leaf, render, sessionManager)
}
