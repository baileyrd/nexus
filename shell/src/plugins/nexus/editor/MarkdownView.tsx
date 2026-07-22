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
import { useEditorStore, type EditorTabMode } from './editorStore.ts'

type RenderFn = (relpath: string | undefined, leafId: string) => ReactElement

/**
 * Per-leaf state that round-trips through `workspaceStore.serialize()`
 * into `.forge/workspace.json`. `relpath` identifies the tab; `mode` /
 * `cursorOffset` / `scrollTop` are #405's restore-on-reopen fields —
 * `getState()` reads them live off the editor store's tab record
 * (kept fresh by `CodeMirrorHost`'s position listener) once the tab
 * exists, and `setState()` parses them out of the persisted layout so
 * `index.ts`'s restore path can seed a freshly-created tab with them
 * before the store has any record for this relpath.
 */
interface MarkdownViewState {
  relpath?: string
  mode?: EditorTabMode
  cursorOffset?: number
  scrollTop?: number
}

const EDITOR_TAB_MODES: readonly EditorTabMode[] = ['live', 'source', 'preview']

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
    const relpath = this.state.relpath
    if (!relpath) return this.state
    // #405 — once the editor store has a tab for this relpath, it is
    // the freshest source of mode / cursor / scroll (CodeMirrorHost's
    // listener keeps it updated live, including for backgrounded
    // tabs — every open tab stays mounted, just `display: none`).
    // Before that (e.g. serialize() racing the initial `loadFile`
    // during restore), fall back to whatever `setState` last parsed
    // out of the persisted layout so a save mid-restore doesn't
    // regress to a blank ephemeral state.
    const tab = useEditorStore.getState().tabs.find((t) => t.relpath === relpath)
    if (!tab) return this.state
    return {
      relpath,
      mode: tab.mode,
      cursorOffset: tab.cursorOffset,
      scrollTop: tab.scrollTop,
    }
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
      const s = state as Record<string, unknown>
      const relpath = typeof s.relpath === 'string' ? s.relpath : undefined
      const mode = EDITOR_TAB_MODES.includes(s.mode as EditorTabMode)
        ? (s.mode as EditorTabMode)
        : undefined
      const cursorOffset = typeof s.cursorOffset === 'number' ? s.cursorOffset : undefined
      const scrollTop = typeof s.scrollTop === 'number' ? s.scrollTop : undefined
      this.state = { relpath, mode, cursorOffset, scrollTop }
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
