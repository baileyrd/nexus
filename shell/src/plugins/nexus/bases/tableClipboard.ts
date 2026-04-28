// BL-031 — registry-of-one for the active bases table's clipboard
// handlers. The plugin's cut/copy/paste commands route to whichever
// table currently owns focus; an unmounted table de-registers itself
// so palette invocations turn into a silent no-op.
//
// Mirrors `activeBases.ts` — the latter dispatches undo/redo onto the
// focused leaf, while this one targets the table inside that leaf.
// They could be one struct, but cell-range selection lives in the
// table component (not BasesView), and only the Table view supports
// the v1 clipboard surface, so a separate handle keeps the contract
// scoped to where the data lives.

export interface TableClipboardHandle {
  cut(): void
  copy(): void
  paste(): void
}

let active: TableClipboardHandle | null = null

export function setActiveTableClipboard(handle: TableClipboardHandle | null): void {
  active = handle
}

export function getActiveTableClipboard(): TableClipboardHandle | null {
  return active
}

/** Run `fn` with the active handle, no-op if none. Returns true when
 *  invoked — symmetric with `withActiveBases`. */
export function withActiveTableClipboard(fn: (h: TableClipboardHandle) => void): boolean {
  if (active) {
    fn(active)
    return true
  }
  return false
}
