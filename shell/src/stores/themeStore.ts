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
import type { KernelAPI } from '../types/plugin'
import { clientLogger } from '../host/clientLogger'

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

// Listing-friendly snippet shape returned by the kernel's
// `get_available_snippets` handler. Mirrors `SnippetMetadata` in
// `crates/nexus-theme/src/api.rs`. Extra fields (mode, scope) are
// passed through opaquely so a kernel-side bump doesn't force a
// shell rebuild — Part 3 only renders id/name/description/enabled.
export interface AvailableSnippet {
  id: string
  name: string
  description: string
  enabled: boolean
  [key: string]: unknown
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
  // Populated by hydrate() from `get_available_snippets`. The Part 3
  // Appearance UI iterates this to render checkboxes + reorder
  // controls. The `enabled` field is authoritative — `enabledSnippets`
  // (above) is the same data flattened to ids in cascade order; both
  // are kept so consumers can pick whichever shape is more ergonomic.
  availableSnippets: AvailableSnippet[]
  activeThemeId: string | null
  // Mirrors the kernel's `ThemeMode` (`'light' | 'dark' | 'system'`).
  // Distinct from the legacy `theme` field, which excludes `'system'`
  // and is written to the DOM density attribute path. `kernelMode` is
  // persisted so the user's `set_mode` choice survives restart and is
  // pushed back to the kernel via `apply_config` on hydrate.
  kernelMode: ThemeMode
  resolvedVariables: Record<string, string>
  enabledSnippets: string[]
  loaded: boolean
  // Track variable names we've inlined onto :root so we can clear
  // them when the kernel reports a smaller set. Without this a
  // snippet toggle would leave orphan vars stuck on :root.
  appliedVariableNames: string[]

  // ── Kernel-driven actions ──────────────────────────────────────────
  //
  // Phase 4.1 narrowing: each action takes the kernel sub-API
  // directly, not the full PluginAPI. The only PluginAPI surface
  // they ever touched was `kernel.invoke(...)`; passing the
  // narrow `KernelAPI` makes the dep visible at every call site.
  hydrate: (kernel: KernelAPI) => Promise<void>
  setActiveTheme: (kernel: KernelAPI, themeId: string) => Promise<void>
  setMode: (kernel: KernelAPI, mode: ThemeMode) => Promise<void>
  toggleSnippet: (kernel: KernelAPI, snippetId: string) => Promise<void>
  // Replace the full ordered list of enabled snippet ids. The kernel
  // emits `com.nexus.theme.changed`, the themeService subscriber
  // re-hydrates, and the new cascade lands on :root automatically.
  setSnippetOrder: (kernel: KernelAPI, orderedIds: string[]) => Promise<void>
  applyResolvedVariables: () => void
}

// Persisted half. Includes the kernel-mirrored selection
// (`activeThemeId`, `kernelMode`, `enabledSnippets`) so the user's
// theme choice survives restart — the kernel theme plugin holds only
// in-memory state, so the shell is the persistence layer. On boot,
// `hydrate()` pushes the persisted snapshot to the kernel via
// `apply_config` before reading back the resolved variables.
type PersistedShape = Pick<
  ThemeStore,
  'theme' | 'density' | 'activeThemeId' | 'kernelMode' | 'enabledSnippets'
>

function applyToDom(_theme: ThemeMode, density: Density) {
  // The legacy `data-theme` attribute is no longer written — the
  // kernel theme drives all color tokens via the `--nx-*` bridge in
  // index.html. We still write `data-density` because the density
  // scale (compact/cozy/spacious) lives in index.html, not the
  // kernel theme registry.
  if (typeof document === 'undefined') return
  document.documentElement.dataset.density = density
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
      // gate UI on it to avoid flashing empty lists. `kernelMode`
      // defaults to `'system'` to match the kernel's `ThemeMode::default()`.
      availableThemes: [],
      availableSnippets: [],
      activeThemeId: null,
      kernelMode: 'system',
      resolvedVariables: {},
      enabledSnippets: [],
      loaded: false,
      appliedVariableNames: [],

      hydrate: async (kernel: KernelAPI) => {
        // Restore the persisted selection into the kernel before
        // reading state back. The kernel's theme plugin only holds
        // in-memory state (`ThemeCorePlugin::with_builtins(...)` in
        // crates/nexus-bootstrap/src/lib.rs) — without this push the
        // engine boots into its hard-coded default and the user's
        // last `setActiveTheme` / `setMode` choice is lost on every
        // restart. `apply_config` (handler 9) silently drops unknown
        // theme/snippet ids, so a stale persist blob can't poison
        // boot.
        const persisted = get()
        if (persisted.activeThemeId) {
          try {
            await kernel.invoke(THEME_PLUGIN_ID, 'apply_config', {
              config: {
                theme_id: persisted.activeThemeId,
                mode: persisted.kernelMode,
                enabled_snippets: persisted.enabledSnippets,
              },
            })
          } catch (err) {
            clientLogger.warn(
              '[themeStore] apply_config (restore) failed; falling back to kernel default',
              err,
            )
          }
        }

        // Three parallel round-trips: the config snapshot, the list of
        // available themes, and the list of snippets. Each one is
        // best-effort — if the `com.nexus.theme` kernel plugin isn't
        // loaded (or returns an error) we degrade to defaults rather
        // than throwing, which would leave `loaded: false` forever and
        // strand the Appearance UI on "Loading…".
        const [config, availableThemes, availableSnippets] = await Promise.all([
          api.kernel
            .invoke<KernelThemeConfig>(THEME_PLUGIN_ID, 'get_theme_config', {})
            .catch((err) => {
              clientLogger.warn(
                '[themeStore] get_theme_config failed; using defaults',
                err,
              )
              return null
            }),
          api.kernel
            .invoke<ThemeManifestEntry[]>(
              THEME_PLUGIN_ID,
              'get_available_themes',
              {},
            )
            .catch(() => [] as ThemeManifestEntry[]),
          api.kernel
            .invoke<AvailableSnippet[]>(
              THEME_PLUGIN_ID,
              'get_available_snippets',
              {},
            )
            .catch(() => [] as AvailableSnippet[]),
        ])

        const variables =
          config !== null
            ? await api.kernel
                .invoke<Record<string, string>>(
                  THEME_PLUGIN_ID,
                  'compute_variables',
                  {
                    theme_id: config.theme_id,
                    enabled_snippets: config.enabled_snippets,
                  },
                )
                .catch(() => ({} as Record<string, string>))
            : ({} as Record<string, string>)

        set({
          activeThemeId: config?.theme_id ?? null,
          kernelMode: config?.mode ?? get().kernelMode,
          enabledSnippets: config?.enabled_snippets ?? [],
          availableThemes,
          availableSnippets,
          resolvedVariables: variables,
          loaded: true,
        })
        get().applyResolvedVariables()
      },

      setActiveTheme: async (kernel: KernelAPI, themeId: string) => {
        // Fire-and-update: the kernel echoes the resulting AppliedTheme
        // so we can apply variables synchronously. The .changed event
        // also fires (and a subscriber will re-hydrate) — that's a
        // safe no-op since the values match.
        const applied = await kernel.invoke<AppliedTheme>(
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

      setMode: async (kernel: KernelAPI, mode: ThemeMode) => {
        await kernel.invoke(THEME_PLUGIN_ID, 'set_mode', { mode })
        // Track the kernel-side mode separately from the legacy
        // `theme` slot — the legacy field can't represent `'system'`,
        // so persisting it alone would lose the user's choice across
        // restart. `kernelMode` is what `hydrate()` ships back to the
        // kernel via `apply_config`.
        set({ kernelMode: mode })
        // The kernel records the mode preference but doesn't auto-pick
        // a theme — the chrome only repaints when the active theme
        // changes. Find the first available theme whose category
        // matches the requested mode and apply it. 'system' resolves
        // via matchMedia. Caller's UI may reflect mode in radio state
        // independently; we only persist the local `theme` slot for
        // backwards-compat with code that still reads it.
        //
        // 'system' is intentionally NOT written to the legacy `theme`
        // slot — older callers reading `theme` expect a concrete
        // 'light' | 'dark' value; the kernel's matchMedia resolver
        // determines the effective theme below.
        if (mode !== 'system') {
          set({ theme: mode })
        }
        if (typeof document === 'undefined') return
        const desired: 'light' | 'dark' =
          mode === 'system'
            ? (typeof window !== 'undefined' &&
              window.matchMedia?.('(prefers-color-scheme: dark)').matches
                ? 'dark'
                : 'light')
            : mode
        const themes = get().availableThemes
        const active = themes.find((t) => t.id === get().activeThemeId)
        const activeCat =
          typeof active?.category === 'string' ? active.category : undefined
        if (activeCat === desired) return
        const match = themes.find((t) => t.category === desired)
        if (match) await get().setActiveTheme(kernel, match.id)
      },

      toggleSnippet: async (kernel: KernelAPI, snippetId: string) => {
        await kernel.invoke<string[]>(THEME_PLUGIN_ID, 'toggle_snippet', {
          id: snippetId,
        })
        // The .changed event will re-hydrate variables; nothing else
        // to do here. We don't optimistically update enabledSnippets
        // because the kernel's order-after-toggle is the source of
        // truth and we'd rather avoid a flicker if it differs.
      },

      setSnippetOrder: async (kernel: KernelAPI, orderedIds: string[]) => {
        // Wire arg name is `ids` (not `ordered_ids`) — see
        // `ReorderSnippetsArgs` in `crates/nexus-theme/src/core_plugin.rs`.
        // The kernel emits `com.nexus.theme.changed` after applying;
        // the themeService subscriber re-hydrates. We don't update
        // local state optimistically: a server-side validation reject
        // (unknown id) would otherwise leave the UI temporarily
        // showing an order the kernel never accepted.
        await kernel.invoke<string[]>(THEME_PLUGIN_ID, 'reorder_snippets', {
          ids: orderedIds,
        })
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
      version: 2,
      // Persist the legacy local fields *and* the kernel-mirrored
      // selection. The kernel theme plugin only holds in-memory state,
      // so without persisting `activeThemeId` / `kernelMode` /
      // `enabledSnippets` here the user's choice resets to the
      // built-in default on every restart. `hydrate()` pushes this
      // snapshot back to the kernel via `apply_config` before reading
      // resolved variables, so the engine's runtime state matches the
      // persisted UI selection.
      partialize: (state): PersistedShape => ({
        theme: state.theme,
        density: state.density,
        activeThemeId: state.activeThemeId,
        kernelMode: state.kernelMode,
        enabledSnippets: state.enabledSnippets,
      }),
      // v1 → v2 migration: v1 only persisted `{theme, density}`. We
      // pass the v1 blob through unchanged — the new kernel-mirrored
      // fields fall back to their store defaults via Zustand's
      // shallow merge (activeThemeId: null, kernelMode: 'system',
      // enabledSnippets: []). A null `activeThemeId` makes `hydrate()`
      // skip the `apply_config` restore path on the first post-upgrade
      // boot, so the kernel's built-in default is used until the user
      // picks a theme. Without this function, a version mismatch
      // would discard `theme`/`density` too.
      migrate: (persisted, _version) => persisted as PersistedShape,
      onRehydrateStorage: () => (state) => {
        if (state) applyToDom(state.theme, state.density)
      },
    }
  )
)
