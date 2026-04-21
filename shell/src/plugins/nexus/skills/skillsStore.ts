import { create } from 'zustand'

/**
 * One skill row, projected from `com.nexus.skills::list`.
 *
 * The kernel returns the full `Skill` struct (frontmatter merged with
 * body) per crates/nexus-skills/src/lib.rs::Skill. We only render
 * what fits in a sidebar row + an inline expand panel; the full
 * `body` is kept so the expanded view doesn't need a second `get`.
 */
export interface SkillEntry {
  id: string
  name: string
  description: string
  version: string
  author: string
  tags: string[]
  applicableContexts: string[]
  triggers: string[]
  body: string
}

interface SkillsStoreState {
  loading: boolean
  loadError: string | null
  skills: SkillEntry[]
  /** Skill id whose row is currently expanded, or null. */
  expandedId: string | null

  setLoading(b: boolean): void
  setLoadError(e: string | null): void
  setSkills(skills: SkillEntry[]): void
  toggleExpanded(id: string): void
  reset(): void
}

export const useSkillsStore = create<SkillsStoreState>((set) => ({
  loading: false,
  loadError: null,
  skills: [],
  expandedId: null,

  setLoading: (b) => set({ loading: b }),
  setLoadError: (e) => set({ loadError: e }),
  setSkills: (skills) => set({ skills }),
  toggleExpanded: (id) =>
    set((s) => ({ expandedId: s.expandedId === id ? null : id })),
  reset: () =>
    set({
      loading: false,
      loadError: null,
      skills: [],
      expandedId: null,
    }),
}))
