// BL-133 follow-up — runtime hook for the Notifications settings tab.
//
// Captures the PluginAPI at activate-time so the React component can
// reach `api.kernel.invoke(...)` without prop-drilling through the
// generic settings tab renderer.

import type { PluginAPI } from '../../../types/plugin'

let _api: PluginAPI | null = null

export function setNotificationsSettingsApi(api: PluginAPI): void {
  _api = api
}

export function getNotificationsSettingsApi(): PluginAPI {
  if (!_api) {
    throw new Error('[nexus.notificationsSettings] api accessed before activate')
  }
  return _api
}
