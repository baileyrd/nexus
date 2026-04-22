// Concrete Leaf implementation — the universal panel container.
// Mirrors Obsidian's `WorkspaceLeaf` (jD) semantics from
// /home/baileyrd/projects/obsidian_reverse/docs/10-editor-shell.md §3.
//
// Framework-agnostic: no React, no Zustand, no workspaceStore imports.
// Event emission is delegated via an optional `emit` callback supplied by
// the constructor; Phase 3 will wire this to `workspaceStore.emit`.

import type { Leaf, View, ViewState, WorkspaceParent } from './types.ts'
import { viewRegistry } from './ViewRegistry.ts'

type EmitFn = (event: string, payload?: unknown) => void

/**
 * Runtime leaf. Implements the Obsidian `setViewState` algorithm literally
 * (see docs/10-editor-shell.md §3; plan §Phase 2 lines 65–72).
 *
 * The leaf may exist in the tree *before* its DOM host is mounted
 * (hydration from persisted layout, or a tab in a not-yet-rendered tab
 * group). In that case `containerEl` is null and `onOpen` is deferred:
 * the pending-open stash is replayed by `attachContainer` when the host
 * eventually mounts. This matches Obsidian's deferred-mount behaviour
 * (plan line 77).
 */
export class LeafImpl implements Leaf {
  readonly id: string
  parent: WorkspaceParent
  view: View | null = null
  containerEl: HTMLElement | null = null
  pinned = false
  group: string | null = null

  // `_pendingOpen` holds the most recent ViewState that arrived while
  // `containerEl` was null — `attachContainer` consumes it.
  private _pendingOpen: { state: ViewState; eState?: unknown } | null = null

  // `_opened` tracks whether `view.onOpen` has already been called on the
  // *current* view instance. Re-mounts after a transient unmount
  // (containerEl set to null then back to non-null without a new
  // setViewState) must NOT re-invoke `onOpen` — the view still owns its
  // DOM from the first mount. See the re-mount edge case covered in
  // Leaf.test.ts.
  private _opened = false

  private readonly emit?: EmitFn

  constructor(parent: WorkspaceParent, emit?: EmitFn) {
    this.id = crypto.randomUUID()
    this.parent = parent
    this.emit = emit
  }

  /**
   * Swap this leaf's view. The single mutation choke-point — serialization,
   * history, drag-drop and popouts all reduce to this call. Follows the
   * 7-step algorithm from docs/10-editor-shell.md §3 / plan lines 65–72.
   */
  async setViewState(state: ViewState, eState?: unknown): Promise<void> {
    // 1. Tear down previous view, if any.
    if (this.view) {
      await this.view.onClose()
      if (this.containerEl) {
        this.containerEl.replaceChildren()
      }
    }

    // 2. Resolve creator, falling back to `empty` — never throw on unknown
    //    types (see plan §Phase 2 resilience note).
    const creator =
      viewRegistry.getCreator(state.type) ?? viewRegistry.getCreator('empty')
    if (!creator) {
      // Phase 1 guarantees 'empty' is always registered at module load.
      throw new Error(
        `[Leaf] no creator for '${state.type}' and no 'empty' fallback — ViewRegistry bug`,
      )
    }

    // 3. Instantiate.
    this.view = creator(this)
    this._opened = false
    this._openedEl = null

    // 4. Hydrate view-specific state.
    if (state.state !== undefined) {
      await this.view.setState(state.state, eState)
    }

    // 5. Mount now if the host is ready; otherwise stash for later replay.
    if (this.containerEl) {
      await this.view.onOpen(this.containerEl)
      this._opened = true
      this._openedEl = this.containerEl
      this._pendingOpen = null
    } else {
      this._pendingOpen = { state, eState }
    }

    // 6. Apply per-leaf flags.
    this.pinned = state.pinned ?? false
    this.group = state.group ?? null

    // 7. Emit events. The plan notes `active-leaf-change` should ultimately
    //    route through `workspaceStore.setActiveLeaf`; Phase 3 will bridge
    //    this emission to the store-level setter.
    this.emit?.('view-changed', { leaf: this })
    if (state.active) {
      this.emit?.('active-leaf-change', { leaf: this })
    }
  }

  /**
   * Serialize this leaf's current state for persistence. `active` is
   * intentionally omitted — it is a workspace-level property recorded
   * against the active-leaf id, not a per-leaf flag.
   */
  getViewState(): ViewState {
    const result: ViewState = {
      type: this.view?.viewType ?? 'empty',
      state: this.view?.getState(),
    }
    if (this.pinned) result.pinned = true
    if (this.group !== null) result.group = this.group
    return result
  }

  /**
   * Permanently close this leaf. Fires `onClose`, clears DOM and view refs.
   * Tree mutation (removing from parent) is the workspace store's job
   * (Phase 3) — see plan line 134.
   */
  async detach(): Promise<void> {
    if (this.view) {
      await this.view.onClose()
      if (this.containerEl) {
        this.containerEl.replaceChildren()
      }
    }
    this.view = null
    this.containerEl = null
    this._pendingOpen = null
    this._opened = false
    this._openedEl = null
  }

  /**
   * Attach or detach the DOM host for this leaf.
   *
   * - Setting `el` non-null: record it. If a pending-open stash exists and
   *   the view hasn't been opened yet, invoke `onOpen(el)` now. If the view
   *   was previously opened into a different element (e.g. the sidebar was
   *   collapsed so React unmounted the old LeafHost, and is now being
   *   re-rendered on reopen), tear down the old mount via `onClose` and
   *   re-mount into the new `el` — the view's DOM lived inside the old
   *   element which is no longer in the tree.
   * - Setting `el` null (unmount): do NOT call `onClose`. Views are only
   *   unloaded on `detach` (plan line 134).
   */
  private _openedEl: HTMLElement | null = null

  async attachContainer(el: HTMLElement | null): Promise<void> {
    this.containerEl = el
    if (el === null) {
      // Transient unmount — view stays alive, onClose deferred to detach.
      return
    }
    if (this.view && this._pendingOpen && !this._opened) {
      await this.view.onOpen(el)
      this._opened = true
      this._openedEl = el
      this._pendingOpen = null
      return
    }
    // Re-attach to a fresh container after a transient unmount (e.g.
    // sidedock collapse/reopen). The view's DOM is in the previous
    // (now-detached) element; re-home it by closing + re-opening.
    if (this.view && this._opened && this._openedEl !== el) {
      await this.view.onClose()
      await this.view.onOpen(el)
      this._openedEl = el
    }
  }
}
