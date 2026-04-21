// src/stores/themeStore.ts
// Theme mode (dark/light) + density preset.
// Writes data-theme / data-density on <html> so CSS token blocks pick up
// the right values; Zustand persist keeps the user's choice across reloads.

import { create } from 'zustand'
import { persist } from 'zustand/middleware'

export type ThemeMode = 'dark' | 'light'
export type Density  = 'compact' | 'cozy' | 'spacious'

interface ThemeStore {
  theme: ThemeMode
  density: Density
  setTheme:   (mode: ThemeMode) => void
  setDensity: (density: Density) => void
  toggleTheme: () => void
}

function applyToDom(theme: ThemeMode, density: Density) {
  const root = document.documentElement
  root.dataset.theme   = theme
  root.dataset.density = density
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
    }),
    {
      name: 'shell-theme',
      version: 1,
      onRehydrateStorage: () => (state) => {
        if (state) applyToDom(state.theme, state.density)
      },
    }
  )
)
