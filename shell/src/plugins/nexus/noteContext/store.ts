// In-memory UI state for the Note Context panel — tracks which
// accordion sections are currently expanded. Lazy-load semantics
// hinge on `isExpanded(id)` returning true/false.
//
// Persistence across sessions is intentionally NOT wired today: the
// configuration schema only supports scalar types (boolean / string /
// number / select / keybinding / password), so a `string[]` of
// section ids has nowhere to land. The default state — `backlinks`
// expanded — covers the common case; if persistence becomes a felt
// need, lift this into per-section boolean config keys.

import { create } from 'zustand'

interface NoteContextState {
  expanded: Set<string>
  setExpanded(ids: Iterable<string>): void
  toggle(id: string): void
  isExpanded(id: string): boolean
}

export const useNoteContextStore = create<NoteContextState>((set, get) => ({
  expanded: new Set<string>(['backlinks']),
  setExpanded(ids) {
    set({ expanded: new Set(ids) })
  },
  toggle(id) {
    const next = new Set(get().expanded)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    set({ expanded: next })
  },
  isExpanded(id) {
    return get().expanded.has(id)
  },
}))
