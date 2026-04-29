// shell/src/plugins/nexus/ai/actions/registry.ts
//
// BL-035 — registry for AI actions surfaced from the editor's
// right-click selection menu and from the block-handle menu (canvas
// node menus follow once the canvas plugin lands BL-038). Mirrors the
// singleton-module pattern used by `contextContributors.ts` so the
// shell can house both sides — the public extension-api surface
// (`api.ai.registerAction`) and the in-tree wiring — under the same
// type contract.
//
// Adapter contract: `run(ctx)` is async + void-returning. Errors a
// handler raises must NEVER propagate up to the menu render — a single
// misbehaving action would otherwise crash the entire context menu and
// strip the user of unrelated surrounding actions. The registry catches
// + console.warns instead.
//
// Lifetime: registrations return idempotent disposers. The AI plugin
// tracks them via `ctx.disposables` so a hot plugin-reload sweeps the
// built-ins without leaving duplicates.

import type {
  AiAction,
  AiActionContext,
  AiActionSurface,
} from '@nexus/extension-api'

class AiActionRegistry {
  private actions: AiAction[] = []

  /** Register a new action. Returns an idempotent disposer; the AI
   *  plugin tracks it through `ctx.disposables` so unloads clean up
   *  automatically. Empty `surfaces` arrays are accepted but the
   *  action will never be surfaced — caller is responsible. */
  register(action: AiAction): () => void {
    const id = action.id?.trim?.() ?? ''
    if (!id) {
      console.warn(`[ai.actions] register: empty id — ignored`)
      return () => {}
    }
    this.actions.push(action)
    let disposed = false
    return () => {
      if (disposed) return
      disposed = true
      const idx = this.actions.indexOf(action)
      if (idx !== -1) this.actions.splice(idx, 1)
    }
  }

  /** Snapshot of registered actions in registration order. Exposed
   *  for tests and debugging; production callers should go through
   *  `actionsForSurface`. */
  list(): ReadonlyArray<AiAction> {
    return this.actions.slice()
  }

  /** Filter the registered actions by surface tag, preserving
   *  registration order. The menu wiring code calls this with the
   *  surface it's rendering for. */
  actionsForSurface(surface: AiActionSurface): AiAction[] {
    return this.actions.filter((a) => a.surfaces.includes(surface))
  }

  /** Invoke an action's `run` callback with the bound surface payload.
   *  Errors are caught + logged so a misbehaving action can't crash
   *  the menu that triggered it. Returns true on success, false when
   *  the action throws. */
  async invoke(action: AiAction, ctx: AiActionContext): Promise<boolean> {
    try {
      await action.run(ctx)
      return true
    } catch (err) {
      console.warn(`[ai.actions] '${action.id}' run threw`, err)
      return false
    }
  }

  /** Test-only — wipe every registration. Production code never
   *  needs this; the disposer pattern handles teardown. */
  _resetForTests(): void {
    this.actions = []
  }
}

export const aiActionRegistry = new AiActionRegistry()
