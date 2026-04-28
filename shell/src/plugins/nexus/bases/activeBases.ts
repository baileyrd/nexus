// Module-level registry for the "currently focused bases leaf". Every
// mounted BasesView registers a handle on focus; the bases plugin's
// commands (undo / redo / cut / copy / paste) dispatch to that
// handle. The global keybinding dispatcher (shell/App.tsx) gates on
// the `bases.focused` context key so chords only fire when a base
// actually owns focus.
//
// A registry-of-one is deliberate: the global dispatcher doesn't know
// which leaf the user intends, so "the leaf whose container is
// currently focused" is the well-defined pick. Mirrors
// canvas/activeCanvas.ts 1:1.
//
// `cut` / `copy` / `paste` are stubs in BL-030; BL-031 fills them in
// with the cell-range clipboard work.

export interface BasesHandle {
  undo(): void
  redo(): void
  cut(): void
  copy(): void
  paste(): void
}

let active: BasesHandle | null = null

export function setActiveBases(handle: BasesHandle | null): void {
  active = handle
}

export function getActiveBases(): BasesHandle | null {
  return active
}

/** Call `fn` only when there's an active bases leaf; silent no-op
 *  otherwise so command-palette invocations without a focused base
 *  don't throw. Returns true when the handle was invoked. */
export function withActiveBases(fn: (h: BasesHandle) => void): boolean {
  if (active) {
    fn(active)
    return true
  }
  return false
}
