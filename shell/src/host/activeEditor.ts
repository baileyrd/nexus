// OI-14 — pure projection helpers backing `api.editor.active()` and the
// `api.editor.onChange()` dedupe path in PluginAPI.ts. Kept in a separate
// file from PluginAPI.ts so unit tests don't drag in `@tauri-apps/*`
// imports just to exercise the projection logic.

import type { ActiveEditor } from '../types/plugin'

export function computeActiveEditor(state: {
  activeRelpath: string | null
  sessionRevision: Map<string, number>
}): ActiveEditor | null {
  const relpath = state.activeRelpath
  if (relpath == null) return null
  const revision = state.sessionRevision.get(relpath) ?? 0
  return { relpath, revision }
}

export function activeEditorEquals(
  a: ActiveEditor | null,
  b: ActiveEditor | null,
): boolean {
  if (a === b) return true
  if (a == null || b == null) return false
  return a.relpath === b.relpath && a.revision === b.revision
}
