// Module-level registry for the "currently focused canvas". Every
// mounted CanvasView registers a handle on focus; the canvas plugin's
// commands (undo / redo / delete / fit / help) dispatch to that
// handle. The global keybinding dispatcher (shell/App.tsx) gates on
// the `canvas.focused` context key so chords only fire when a canvas
// actually owns focus.
//
// A registry-of-one is deliberate: the global dispatcher doesn't know
// which leaf the user intends, so "the leaf whose container is
// currently focused" is the well-defined pick.

export interface CanvasHandle {
  undo(): void
  redo(): void
  deleteSelected(): void
  fit(): void
  fitSelection(): void
  toggleHelp(): void
  closeHelp(): void
  toggleGrid(): void
  toggleBackgroundInspector(): void
  tidy(): void
  exportPng(): void
  exportSvg(): void
  exportPdf(): void
  /** Multiplicative zoom around the viewport centre. */
  zoomBy(factor: number): void
  /** Reset camera to zoom = 1, anchored on viewport centre. */
  resetZoom(): void
  /** Insert a blank text node at world coordinates (default: viewport centre). */
  addBlankCard(world?: { x: number; y: number }): void
}

let active: CanvasHandle | null = null

export function setActiveCanvas(handle: CanvasHandle | null): void {
  active = handle
}

export function getActiveCanvas(): CanvasHandle | null {
  return active
}

/** Call `fn` only when there's an active canvas; silent no-op
 *  otherwise so command-palette invocations without a focused canvas
 *  don't throw. */
export function withActiveCanvas(fn: (h: CanvasHandle) => void): void {
  if (active) fn(active)
}
