// BL-077 follow-up — save-format hook registry.
//
// Every code-mode tab with a live LSP extension registers a format
// hook keyed by `relpath` here. The shell's `nexus.editor.save`
// command awaits the hook for the active tab before writing, so
// vim-style `:w`, custom save chords, and any future save trigger
// all get the same format-on-save behaviour as the CM6 `Mod-s`
// keymap.
//
// Rationale: the LSP extension lives inside CM6 (it needs the
// `EditorView`); the save command lives in the shell `commands`
// surface and doesn't have CM6 in scope. This module is the shared
// blackboard between them.
//
// Hooks are stored module-level — there is exactly one open editor
// per relpath at any moment (the workspace's tab uniqueness
// invariant), so the keying is collision-free.

type FormatHook = () => Promise<void>

const hooks = new Map<string, FormatHook>()

/** Register a format hook for a given forge-relative path. The
 *  returned disposer removes the hook iff it's still the registered
 *  one (a later `register` for the same relpath wins; the older
 *  disposer becomes a no-op). */
export function registerSaveFormatHook(
  relpath: string,
  hook: FormatHook,
): () => void {
  hooks.set(relpath, hook)
  return () => {
    if (hooks.get(relpath) === hook) {
      hooks.delete(relpath)
    }
  }
}

/** Run the registered format hook for `relpath`, if any. Resolves
 *  silently when no hook is registered (most tabs don't have one);
 *  swallows hook errors so a failing format never blocks save. The
 *  caller logs the error if it cares. */
export async function runSaveFormatHook(
  relpath: string,
  onError?: (err: unknown) => void,
): Promise<void> {
  const hook = hooks.get(relpath)
  if (!hook) return
  try {
    await hook()
  } catch (err) {
    onError?.(err)
  }
}

/** Test-only — wipe the registry between tests. */
export function _resetSaveFormatHooksForTests(): void {
  hooks.clear()
}

/** Test-only — report the current registration count. */
export function _saveFormatHookCount(): number {
  return hooks.size
}
