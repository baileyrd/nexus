// src/plugins/core/themeService/index.ts
// Core service plugin — kernel theme bridge.
//
// Owns the *subscription* to `com.nexus.theme.changed` and the
// initial hydrate from the kernel's theme engine. The store
// (`stores/themeStore.ts`) owns state + DOM application; this
// plugin is a thin lifecycle wrapper around it so the kernel
// subscription is set up exactly once, at boot, with the
// PluginRegistry-managed unsub teardown.
//
// Replaces the prior in-process `ThemeService` (which carried
// brand-named built-in palettes and auto-flipped on OS prefs).
// All theme data now flows from `crates/nexus-theme` via the
// `com.nexus.theme` kernel plugin.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import {
  THEME_CHANGED_EVENT,
  useThemeStore,
  type KernelThemeConfig,
} from '../../../stores/themeStore'

export const themeServicePlugin: Plugin = {
  manifest: {
    id: 'core.theme-service',
    name: 'Theme Service',
    version: '2.0.0',
    core: true,
    activationEvents: ['onStartup'],
    contributes: {},
  },

  async activate(api: PluginAPI) {
    // The kernel only finishes booting after `nexus.workspace` opens a
    // forge. This plugin runs eagerly at startup, so we hydrate iff
    // the kernel is already up (e.g. persisted-workspace path where
    // `workspace:opened` fired before our listener registered) and
    // also subscribe to `workspace:opened` for the cold-start path.
    const tryHydrate = async () => {
      if (!(await api.kernel.available())) return
      try {
        await useThemeStore.getState().hydrate(api)
      } catch (err) {
        console.warn(
          '[core.theme-service] hydrate failed; using shell.css defaults',
          err,
        )
      }
    }
    await tryHydrate()

    api.events.on('workspace:opened', () => {
      void tryHydrate()
    })

    // Subscribe with the prefix — handler routes by topic. The
    // payload IS the ThemeConfig snapshot (theme_id, mode,
    // enabled_snippets) per crates/nexus-theme/src/core_plugin.rs,
    // but it does NOT carry resolved variables. We re-hydrate to
    // pick those up — one extra round-trip per change, acceptable
    // given how rare theme mutations are.
    //
    // PluginRegistry auto-cleans the unsub on plugin deactivate
    // (Phase 1 wiring), so we don't track it manually.
    try {
      await api.kernel.on<KernelThemeConfig>(THEME_CHANGED_EVENT, (topic) => {
        if (topic !== THEME_CHANGED_EVENT) return
        // Re-hydrate. `compute_variables` is cheap and idempotent;
        // the store no-ops if values match.
        void useThemeStore.getState().hydrate(api)
      })
    } catch (err) {
      console.warn(
        '[core.theme-service] subscribe to com.nexus.theme.changed failed',
        err,
      )
    }

    console.info('[core.theme-service] kernel-sync ready')
  },
}
