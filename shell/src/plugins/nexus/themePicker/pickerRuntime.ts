// Module-scope handle for the theme-picker components, set at
// activate-time so they can reach the kernel + platform surfaces
// without prop drilling through `api.views.register`-rendered
// components.
//
// Phase 4.1 narrowing: the stored handle's type is `ThemePickerApi`,
// a `Pick<PluginAPI, 'kernel' | 'platform'>` slice. The themePicker
// only uses two PluginAPI surfaces today:
//   - `kernel.invoke(...)` for `com.nexus.theme::{compute_variables,
//     apply_theme, set_plugin_overrides, reload}` and (via
//     useThemeStore actions) `set_mode`, `toggle_snippet`, and
//     `reorder_snippets`.
//   - `platform.dialog.saveFile` + `platform.fs.{mkdir, writeText}`
//     for the ThemeBuilder "Save to disk" flow.
//
// Narrowing the type makes those two surfaces visible at a glance
// without grepping ThemePicker.tsx / ThemeBuilder.tsx. The deeper
// "expose a typed wrapper per kernel handler" pattern used elsewhere
// (notificationsSettings, recall, search) doesn't fit here as well —
// the components also pass the kernel sub-API directly to themeStore
// actions, and inventing wrappers around store-bound calls would
// require a parallel set of methods on this module.

import type { PluginAPI } from '../../../types/plugin'

/**
 * Slice of PluginAPI that the theme-picker components touch. Keep
 * this in sync with the kernel + platform calls in ThemePicker.tsx
 * and ThemeBuilder.tsx.
 */
export type ThemePickerApi = Pick<PluginAPI, 'kernel' | 'platform'>

let _api: ThemePickerApi | null = null

export function setPickerApi(api: PluginAPI): void {
  // Pin the narrow slice so consumers can't accidentally widen back
  // out to PluginAPI via the singleton.
  _api = { kernel: api.kernel, platform: api.platform }
}

export function getPickerApi(): ThemePickerApi {
  if (!_api) throw new Error('[nexus.themePicker] api accessed before activate')
  return _api
}
