// src/stores/themeStore.ts
// Theme mode (dark/light) + density preset, plus kernel-driven theme state.
//
// Two layers live in this store:
//
//  1. Legacy local-only fields (`theme`, `density`) — drive the
//     `data-theme` / `data-density` attributes on <html> so the
//     Forge token blocks in shell.css pick up the right values.
//     Persisted via Zustand persist; preserved here because main.tsx
//     and any not-yet-migrated callers still read them.
//
//  2. Kernel-mirrored state (`activeThemeId`, `resolvedVariables`,
//     `enabledSnippets`, `availableThemes`) — populated from the
//     `com.nexus.theme` plugin via `hydrate()` and kept in sync via
//     the `com.nexus.theme.changed` event. All mutating actions
//     delegate to the kernel; the store is a *reflection* of kernel
//     state, never the source of truth.
//
// `applyResolvedVariables()` writes the cascade onto :root via
// `setProperty` so CSS variables actually take effect — the legacy
// shell.css :root defaults remain in place to avoid a flash of
// unstyled content if the kernel is slow to hydrate.

import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import type { PluginAPI } from '../types/plugin'

// Wire shapes — mirror the Rust DTOs in crates/nexus-theme. Kept
// loose (`Record<string, string>` for variables, optional snippet
// fields) so a kernel-side schema bump that adds fields doesn't
// require a shell rebuild.
export type ThemeMode = 'dark' | 'light' | 'system'
export type Density  = 'compact' | 'cozy' | 'spacious'

export interface ThemeManifestEntry {
  id: string
  name: string
  // Other ThemeMetadata fields (description, author, …) are passed
  // through opaquely; the settings UI (Part 3) will type them.
  [key: string]: unknown
}

export interface KernelThemeConfig {
  theme_id: string
  mode: ThemeMode
  enabled_snippets: string[]
}

export interface AppliedTheme {
  id: string
  name: string
  variables: Record<string, string>
}

export const THEME_PLUGIN_ID = 'com.nexus.theme'
export const THEME_CHANGED_EVENT = 'com.nexus.theme.changed'

interface ThemeStore {
  // ── Legacy local-only state (preserved for back-compat) ────────────
  theme: ThemeMode
  density: Density
  setTheme:   (mode: ThemeMode) => void
  setDensity: (density: Density) => void
  toggleTheme: () => void

  // ── Kernel-mirrored state ──────────────────────────────────────────
  availableThemes: ThemeManifestEntry[]
  activeThemeId: string | null
  resolvedVariables: Record<string, string>
  enabledSnippets: string[]
  loaded: boolean
  // Track variable names we've inlined onto :root so we can clear
  // them when the kernel reports a smaller set. Without this a
  // snippet toggle would leave orphan vars stuck on :root.
  appliedVariableNames: string[]

  // ── Kernel-driven actions ──────────────────────────────────────────
  hydrate: (api: PluginAPI) => Promise<void>
  setActiveTheme: (api: PluginAPI, themeId: string) => Promise<void>
  setMode: (api: PluginAPI, mode: ThemeMode) => Promise<void>
  toggleSnippet: (api: PluginAPI, snippetId: string) => Promise<void>
  applyResolvedVariables: () => void
}

// Persisted half (legacy fields only — kernel state is rehydrated
// fresh from the engine on every boot).
type PersistedShape = Pick<ThemeStore, 'theme' | 'density'>

function applyToDom(theme: ThemeMode, density: Density) {
  // No-op in non-DOM environments (node:test). The kernel-sync
  // path is unit-tested separately; live DOM application is e2e.
  if (typeof document === 'undefined') return
  const root = document.documentElement
  // 'system' has no concrete CSS attr — fall back to the resolver's
  // last decision (data-theme is overwritten by kernel hydrate too).
  if (theme !== 'system') root.dataset.theme = theme
  root.dataset.density = density
}

// Push the resolved variable cascade onto :root. Cheap; Chromium
// applies the new cascade on the next paint. Variables not present
// in the new map but previously applied get cleared so a snippet
// removal doesn't leave a ghost token behind.
function writeVariablesToRoot(
  vars: Record<string, string>,
  previouslyApplied: string[],
): string[] {
  // Skip silently when no DOM is present (test/SSR environments).
  if (typeof document === 'undefined') return Object.keys(vars)
  const root = document.documentElement
  const next = Object.keys(vars)
  const nextSet = new Set(next)
  for (const name of previouslyApplied) {
    if (!nextSet.has(name)) root.style.removeProperty(name)
  }
  for (const [name, value] of Object.entries(vars)) {
    if (typeof value === 'string') root.style.setProperty(name, value)
  }
  return next
}

export const useThemeStore = create<ThemeStore>()(
  persist(
    (set, get) => ({
      theme: 'dark',
      density: 'cozy',

      setTheme:   (theme)   => { applyToDom(theme,       get().density); set({ theme }) },
      setDensity: (density) => { applyToDom(get().theme, density);       set({ density }) },
      toggleTheme: () => {
        const next: ThemeMode = get().theme === 'dark' ? 'light' : 'dark'
        applyToDom(next, get().density)
        set({ theme: next })
      },

      // Kernel-mirrored defaults. `loaded` flips true once hydrate()
      // has populated from the engine at least once; consumers can
      // gate UI on it to avoid flashing empty lists.
      availableThemes: [],
      activeThemeId: null,
      resolvedVariables: {},
      enabledSnippets: [],
      loaded: false,
      appliedVariableNames: [],

      hydrate: async (api: PluginAPI) => {
        // Two round-trips: the config snapshot (cheap) and the list
        // of available themes (settings UI consumes it). The
        // `compute_variables` call resolves the cascade for the
        // currently-active theme + snippets; this is what actually
        // populates the :root variable map.
        const [config, availableThemes] = await Promise.all([
          api.kernel.invoke<KernelThemeConfig>(
            THEME_PLUGIN_ID,
            'get_theme_config',
            {},
          ),
          api.kernel
            .invoke<ThemeManifestEntry[]>(
              THEME_PLUGIN_ID,
              'get_available_themes',
              {},
            )
            .catch(() => [] as ThemeManifestEntry[]),
        ])

        const variables = await api.kernel
          .invoke<Record<string, string>>(THEME_PLUGIN_ID, 'compute_variables', {
            theme_id: config.theme_id,
            enabled_snippets: config.enabled_snippets,
          })
          .catch(() => ({} as Record<string, string>))

        set({
          activeThemeId: config.theme_id,
          enabledSnippets: config.enabled_snippets,
          availableThemes,
          resolvedVariables: variables,
          loaded: true,
        })
        get().applyResolvedVariables()
      },

      setActiveTheme: async (api: PluginAPI, themeId: string) => {
        // Fire-and-update: the kernel echoes the resulting AppliedTheme
        // so we can apply variables synchronously. The .changed event
        // also fires (and a subscriber will re-hydrate) — that's a
        // safe no-op since the values match.
        const applied = await api.kernel.invoke<AppliedTheme>(
          THEME_PLUGIN_ID,
          'apply_theme',
          { id: themeId },
        )
        set({
          activeThemeId: applied.id,
          resolvedVariables: applied.variables,
        })
        get().applyResolvedVariables()
      },

      setMode: async (api: PluginAPI, mode: ThemeMode) => {
        await api.kernel.invoke(THEME_PLUGIN_ID, 'set_mode', { mode })
        // No echo body to apply directly — wait for the .changed event
        // to re-hydrate, but also nudge the legacy local field so the
        // existing data-theme attr cascade behaves sensibly in the
        // meantime.
        if (mode !== 'system') {
          applyToDom(mode, get().density)
          set({ theme: mode })
        }
      },

      toggleSnippet: async (api: PluginAPI, snippetId: string) => {
        await api.kernel.invoke<string[]>(THEME_PLUGIN_ID, 'toggle_snippet', {
          id: snippetId,
        })
        // The .changed event will re-hydrate variables; nothing else
        // to do here. We don't optimistically update enabledSnippets
        // because the kernel's order-after-toggle is the source of
        // truth and we'd rather avoid a flicker if it differs.
      },

      applyResolvedVariables: () => {
        const { resolvedVariables, appliedVariableNames } = get()
        const next = writeVariablesToRoot(
          resolvedVariables,
          appliedVariableNames,
        )
        set({ appliedVariableNames: next })
      },
    }),
    {
      name: 'shell-theme',
      version: 1,
      // Persist only the legacy local fields. Kernel-mirrored state
      // is fetched fresh on boot — persisting it would risk drift
      // from the on-disk theme config.
      partialize: (state): PersistedShape => ({
        theme: state.theme,
        density: state.density,
      }),
      onRehydrateStorage: () => (state) => {
        if (state) applyToDom(state.theme, state.density)
      },
    }
  )
)
