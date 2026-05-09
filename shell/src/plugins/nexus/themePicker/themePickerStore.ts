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

export type PickerTab = 'themes' | 'snippets' | 'build'

interface ThemePickerState {
  visible: boolean
  activeTab: PickerTab
  query: string
  categoryFilter: CategoryFilter
  /** themeId → resolved variables (subset: SWATCH_KEYS). Cached on open. */
  swatchCache: Record<string, Record<string, string>>
  loadingSwatches: boolean

  // ── Theme builder ──────────────────────────────────────────────────
  /** Which theme the builder is starting from. null = use active theme. */
  builderBaseThemeId: string | null
  /** Variable overrides for single mode (or light column in dual mode). */
  builderOverrides: Record<string, string>
  builderThemeName: string
  builderThemeAuthor: string
  // Dual-mode state
  builderDualMode: boolean
  builderLightOverrides: Record<string, string>
  builderDarkOverrides: Record<string, string>
  builderHueLock: boolean
  builderActiveColumn: 'light' | 'dark'
  /** BL-068 Phase 4 — when true, renders the split-view preview pane
   *  alongside the variable editor. Persisted in the store (rather
   *  than as local React state) so ThemePicker can widen the modal
   *  to match. */
  builderShowPreview: boolean

  open(tab?: PickerTab): void
  close(): void
  setActiveTab(tab: PickerTab): void
  setQuery(q: string): void
  setCategoryFilter(cat: CategoryFilter): void
  setSwatchCache(cache: Record<string, Record<string, string>>): void
  setLoadingSwatches(loading: boolean): void
  setBuilderBaseThemeId(id: string | null): void
  setBuilderOverride(key: string, value: string): void
  clearBuilderOverride(key: string): void
  resetBuilderOverrides(): void
  setBuilderThemeName(name: string): void
  setBuilderThemeAuthor(author: string): void
  setBuilderDualMode(on: boolean): void
  setBuilderLightOverride(key: string, value: string): void
  clearBuilderLightOverride(key: string): void
  setBuilderDarkOverride(key: string, value: string): void
  clearBuilderDarkOverride(key: string): void
  setBuilderHueLock(on: boolean): void
  setBuilderActiveColumn(col: 'light' | 'dark'): void
  setBuilderShowPreview(on: boolean): void
}

export const useThemePickerStore = create<ThemePickerState>((set) => ({
  visible: false,
  activeTab: 'themes',
  query: '',
  categoryFilter: 'all',
  swatchCache: {},
  loadingSwatches: false,

  builderBaseThemeId: null,
  builderOverrides: {},
  builderThemeName: '',
  builderThemeAuthor: '',
  builderDualMode: false,
  builderLightOverrides: {},
  builderDarkOverrides: {},
  builderHueLock: false,
  builderActiveColumn: 'light',
  builderShowPreview: false,

  open: (tab = 'themes') => set({ visible: true, activeTab: tab, query: '', categoryFilter: 'all' }),
  close: () => set({ visible: false }),
  setActiveTab: (tab) => set({ activeTab: tab }),
  setQuery: (q) => set({ query: q }),
  setCategoryFilter: (cat) => set({ categoryFilter: cat }),
  setSwatchCache: (cache) => set({ swatchCache: cache }),
  setLoadingSwatches: (loading) => set({ loadingSwatches: loading }),
  setBuilderBaseThemeId: (id) => set({ builderBaseThemeId: id }),
  setBuilderOverride: (key, value) =>
    set((s) => ({ builderOverrides: { ...s.builderOverrides, [key]: value } })),
  clearBuilderOverride: (key) =>
    set((s) => {
      const next = { ...s.builderOverrides }
      delete next[key]
      return { builderOverrides: next }
    }),
  resetBuilderOverrides: () => set({
    builderOverrides: {},
    builderLightOverrides: {},
    builderDarkOverrides: {},
  }),
  setBuilderThemeName: (name) => set({ builderThemeName: name }),
  setBuilderThemeAuthor: (author) => set({ builderThemeAuthor: author }),
  setBuilderDualMode: (on) => set({ builderDualMode: on }),
  setBuilderLightOverride: (key, value) =>
    set((s) => ({ builderLightOverrides: { ...s.builderLightOverrides, [key]: value } })),
  clearBuilderLightOverride: (key) =>
    set((s) => {
      const next = { ...s.builderLightOverrides }
      delete next[key]
      return { builderLightOverrides: next }
    }),
  setBuilderDarkOverride: (key, value) =>
    set((s) => ({ builderDarkOverrides: { ...s.builderDarkOverrides, [key]: value } })),
  clearBuilderDarkOverride: (key) =>
    set((s) => {
      const next = { ...s.builderDarkOverrides }
      delete next[key]
      return { builderDarkOverrides: next }
    }),
  setBuilderHueLock: (on) => set({ builderHueLock: on }),
  setBuilderActiveColumn: (col) => set({ builderActiveColumn: col }),
  setBuilderShowPreview: (on) => set({ builderShowPreview: on }),
}))
