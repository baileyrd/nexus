import { create } from 'zustand'

export type CategoryFilter =
  | 'all'
  | 'light'
  | 'dark'
  | 'sepia'
  | 'high-contrast'
  | 'custom'

// Typed view of the ThemeMetadata DTO returned by the kernel.
// Mirrors the Rust `ThemeMetadata` in crates/nexus-theme/src/api.rs.
export interface ThemeCard {
  id: string
  name: string
  author: string
  description: string
  /** Kebab-case category string serialised from `ThemeCategory`. */
  category: CategoryFilter
  builtin: boolean
  keywords: string[]
}

// The 5 CSS variable keys we pull per-theme for the swatch strip.
export const SWATCH_KEYS = [
  '--nx-bg-secondary',
  '--nx-bg-primary',
  '--nx-color-primary',
  '--nx-color-secondary',
  '--nx-syntax-keyword',
] as const

interface ThemePickerState {
  visible: boolean
  query: string
  categoryFilter: CategoryFilter
  /** themeId → resolved variables (subset: SWATCH_KEYS). Cached on open. */
  swatchCache: Record<string, Record<string, string>>
  loadingSwatches: boolean

  open(): void
  close(): void
  setQuery(q: string): void
  setCategoryFilter(cat: CategoryFilter): void
  setSwatchCache(cache: Record<string, Record<string, string>>): void
  setLoadingSwatches(loading: boolean): void
}

export const useThemePickerStore = create<ThemePickerState>((set) => ({
  visible: false,
  query: '',
  categoryFilter: 'all',
  swatchCache: {},
  loadingSwatches: false,

  open: () => set({ visible: true, query: '', categoryFilter: 'all' }),
  close: () => set({ visible: false }),
  setQuery: (q) => set({ query: q }),
  setCategoryFilter: (cat) => set({ categoryFilter: cat }),
  setSwatchCache: (cache) => set({ swatchCache: cache }),
  setLoadingSwatches: (loading) => set({ loadingSwatches: loading }),
}))
