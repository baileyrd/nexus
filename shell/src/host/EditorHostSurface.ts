// R10 / #193 — host-owned seam that inverts the host→editor-plugin
// dependency.
//
// Before: `PluginAPI.ts` statically imported `useEditorStore` and
// `fencedCodeRegistry` from the editor plugin, so the entire `PluginAPI`
// module (and therefore *every* `api.*` surface) failed to load if the
// editor plugin module was absent. The host hard-depended on a plugin.
//
// After: the editor plugin *registers* a small adapter here during its
// `activate()`, and `api.editor` delegates to whatever is registered.
// The host no longer imports the editor plugin; the dependency arrow now
// points plugin → host (the correct microkernel direction). When the
// editor plugin is absent the seam degrades predictably:
//   - reads (`getActiveEditor`) return `null` — semantically "no editor,
//     nothing active",
//   - subscriptions never fire,
//   - registrations (`registerFencedCodeRenderer`) fail loudly so a
//     caller can't silently lose a contribution.
//
// Only the three operations `api.editor` actually needs from the editor
// plugin live here. `api.editor.registerSnippet` already routes through
// the host-side `PluginRegistry` (`registry.snippets`) and so was never
// coupled to the editor plugin.

import type { ActiveEditor, FencedRenderer } from '../types/plugin'
import { clientLogger } from './clientLogger'

/**
 * The editor-plugin-provided surface the host's `api.editor` delegates
 * to. The editor plugin builds this in `activate()` over its own
 * `useEditorStore` + `fencedCodeRegistry`; the host never sees those
 * internals.
 */
export interface EditorHostSurface {
  /** Snapshot of the active editor tab, or `null` when none is open. */
  getActiveEditor(): ActiveEditor | null
  /**
   * Subscribe to active-editor changes. The provider is responsible for
   * de-duping (only invoking `handler` when the projected
   * {@link ActiveEditor} actually changes) and must NOT fire `handler`
   * synchronously on subscribe. Returns an unsubscribe function.
   */
  subscribeActiveEditor(handler: (active: ActiveEditor | null) => void): () => void
  /**
   * Register a fenced-code-block renderer for `language`. Returns a
   * disposer. Mirrors the editor plugin's `fencedCodeRegistry.register`.
   */
  registerFencedCodeRenderer(language: string, renderer: FencedRenderer): () => void
}

let surface: EditorHostSurface | null = null

/**
 * Register the editor host surface. Called once by the editor plugin
 * during `activate()`. Returns a disposer that clears the registration
 * (idempotent; only clears if `s` is still the active surface).
 */
export function registerEditorHostSurface(s: EditorHostSurface): () => void {
  if (surface !== null) {
    clientLogger.warn(
      '[EditorHostSurface] a surface is already registered; replacing it. ' +
        'This usually means the editor plugin activated twice.',
    )
  }
  surface = s
  return () => {
    if (surface === s) surface = null
  }
}

/** The registered surface, or `null` when the editor plugin is absent. */
export function getEditorHostSurface(): EditorHostSurface | null {
  return surface
}

/** `true` iff an editor host surface is currently registered. */
export function hasEditorHostSurface(): boolean {
  return surface !== null
}

/**
 * Test-only reset so unit tests can exercise the absent-surface paths
 * without leaking state across cases.
 */
export function __resetEditorHostSurfaceForTests(): void {
  surface = null
}
