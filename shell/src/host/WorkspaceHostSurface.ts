// V16 / repo-review-2026-06-10 — host-owned seam that inverts the
// host→workspace-plugin dependency. Mirrors `EditorHostSurface.ts`
// (R10 / #193), which solved the identical problem for the editor plugin.
//
// Before: host code (`shell/src/workspace/workspaceStore.ts`,
// `ForgeSelector.tsx`, `RightPanelFooter.tsx`) statically imported
// `useWorkspaceStore` from `plugins/nexus/workspace/workspaceStore`, so the
// shell chrome — including `App.tsx` via its `../workspace` imports —
// hard-depended on a specific plugin's zustand store. The dependency arrow
// pointed host → plugin, violating the "shell starts empty" principle
// (architecture-adherence.md §S-E).
//
// After: the workspace plugin *registers* a small adapter here during its
// `activate()`, and host consumers delegate to whatever is registered. The
// host no longer imports the workspace plugin; the arrow points
// plugin → host (the correct microkernel direction). When the workspace
// plugin is absent the seam degrades predictably:
//   - reads (`getWorkspaceRootPath`) return `null` — semantically "no
//     forge open", the same value the plugin's own store starts with,
//   - subscriptions attach lazily: `subscribeWorkspaceRootPath` watches
//     for a (re-)registration and binds to the new surface when it lands,
//     so a consumer that mounts before the plugin activates still starts
//     receiving updates once it does. (Strictly better than the editor
//     seam's never-fires fallback — root-path consumers are chrome that
//     mounts before plugin activation finishes, so late binding is the
//     normal path, not defense-in-depth.)
//
// Only the two operations host chrome actually needs live here: a
// `rootPath` snapshot and a change subscription. Everything else the
// workspace plugin owns (open/close commands, kernel boot) stays behind
// commands and context keys (`nexus.workspace.rootPath`).

/**
 * The workspace-plugin-provided surface host chrome delegates to. The
 * workspace plugin builds this in `activate()` over its own
 * `useWorkspaceStore`; the host never sees that store.
 */
export interface WorkspaceHostSurface {
  /** Absolute path of the open forge root, or `null` when none is open. */
  getRootPath(): string | null
  /**
   * Subscribe to root-path changes. The provider is responsible for
   * de-duping (only invoking `handler` when `rootPath` actually changes)
   * and must NOT fire `handler` synchronously on subscribe. Returns an
   * unsubscribe function.
   */
  subscribeRootPath(handler: (rootPath: string | null) => void): () => void
}

import { clientLogger } from './clientLogger'

let surface: WorkspaceHostSurface | null = null

/**
 * Registration listeners — `subscribeWorkspaceRootPath` uses these to
 * re-bind when the plugin (re-)registers after a consumer subscribed.
 */
const registrationListeners = new Set<() => void>()

function notifyRegistrationChanged(): void {
  for (const listener of registrationListeners) listener()
}

/**
 * Register the workspace host surface. Called once by the workspace
 * plugin during `activate()`. Returns a disposer that clears the
 * registration (idempotent; only clears if `s` is still the active
 * surface) — mirrors `registerEditorHostSurface`.
 */
export function registerWorkspaceHostSurface(s: WorkspaceHostSurface): () => void {
  if (surface !== null) {
    clientLogger.warn(
      '[WorkspaceHostSurface] a surface is already registered; replacing it. ' +
        'This usually means the workspace plugin activated twice.',
    )
  }
  surface = s
  notifyRegistrationChanged()
  return () => {
    if (surface === s) {
      surface = null
      notifyRegistrationChanged()
    }
  }
}

/** The registered surface, or `null` when the workspace plugin is absent. */
export function getWorkspaceHostSurface(): WorkspaceHostSurface | null {
  return surface
}

/** `true` iff a workspace host surface is currently registered. */
export function hasWorkspaceHostSurface(): boolean {
  return surface !== null
}

/**
 * Snapshot of the open forge root. `null` when no forge is open OR the
 * workspace plugin is absent — both mean "nothing to read from".
 */
export function getWorkspaceRootPath(): string | null {
  return surface?.getRootPath() ?? null
}

/**
 * Subscribe to root-path changes through the seam. Unlike calling
 * `surface.subscribeRootPath` directly, this survives the surface being
 * registered *after* the subscription was made (and re-registration):
 * the value subscription is torn down and re-bound whenever the
 * registration changes, and `onChange` fires so callers re-read the
 * snapshot. Shaped for `useSyncExternalStore(subscribe, getSnapshot)`.
 */
export function subscribeWorkspaceRootPath(onChange: () => void): () => void {
  let valueUnsub: (() => void) | null =
    surface?.subscribeRootPath(() => onChange()) ?? null
  const regListener = () => {
    valueUnsub?.()
    valueUnsub = surface?.subscribeRootPath(() => onChange()) ?? null
    // The snapshot may have jumped when the surface appeared/vanished.
    onChange()
  }
  registrationListeners.add(regListener)
  return () => {
    registrationListeners.delete(regListener)
    valueUnsub?.()
    valueUnsub = null
  }
}

/**
 * Test-only reset so unit tests can exercise the absent-surface paths
 * without leaking state across cases.
 */
export function __resetWorkspaceHostSurfaceForTests(): void {
  surface = null
  registrationListeners.clear()
}
