// src/stores/docStore.ts
// Broadcast channel between the active Doc view and the right-panel
// inspector (Outline tab, eventually backlinks/graph too).

import { create } from 'zustand'
import type { Heading } from '../plugins/core/editorArea/MarkdownDoc'

interface DocStore {
  headings: Heading[]
  activeHeading: string | null
  setHeadings: (h: Heading[]) => void
  setActiveHeading: (id: string | null) => void
  jumpToHeading: (id: string) => void
}

export const useDocStore = create<DocStore>((set) => ({
  headings: [],
  activeHeading: null,
  setHeadings:      (headings) => set({ headings }),
  setActiveHeading: (activeHeading) => set({ activeHeading }),
  jumpToHeading: (id) => {
    const el = document.getElementById(id)
    if (!el) return
    el.scrollIntoView({ behavior: 'smooth', block: 'start' })
    set({ activeHeading: id })
  },
}))
