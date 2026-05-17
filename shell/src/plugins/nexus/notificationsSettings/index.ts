// BL-133 follow-up — Notifications settings tab plugin.
//
// Registers a Settings → Notifications tab that surfaces per-channel
// credential entry (Discord webhook, Telegram bot token + chat id,
// SMTP host/port/user/password/recipient) backed by the
// nexus-security keyring. "Send test" buttons dispatch
// `com.nexus.notifications::send` directly so the user can verify the
// round-trip without waiting for a producer event.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { NotificationsSettings } from './NotificationsSettings'
import { setNotificationsSettingsApi } from './notificationsSettingsRuntime'

const TAB_ID = 'notifications'

export const notificationsSettingsPlugin: Plugin = {
  manifest: {
    id: 'nexus.notificationsSettings',
    name: 'Notifications Settings',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    popoutCompatible: false,
    contributes: {
      settingsTabs: [
        {
          id: TAB_ID,
          title: 'Notifications',
          group: 'options',
          priority: 50,
        },
      ],
    },
  },

  activate(api: PluginAPI) {
    setNotificationsSettingsApi(api)
    api.settings.registerTab(TAB_ID, NotificationsSettings, {
      title: 'Notifications',
      group: 'options',
      priority: 50,
    })
  },
}
