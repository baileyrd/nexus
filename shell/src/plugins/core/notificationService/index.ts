// src/plugins/core/notificationService/index.ts
// Service plugin — bootstraps the notification queue and renders the
// in-app toast stack. After this activates, api.notifications.show() is
// available and its notifications are painted by NotificationToaster in
// the overlay slot.

import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { clientLogger } from '../../../clientLogger'
import {
  notificationQueue,
  DEFAULT_NOTIFICATION_DURATION_MS,
  CONFIG_KEY_DURATION,
} from './notificationQueue'
import { NotificationToaster } from './NotificationToaster'

const TOASTER_VIEW_ID = 'core.notification-service.toaster'

export const notificationServicePlugin: Plugin = {
  manifest: {
    id: 'core.notification-service',
    name: 'Notification Service',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    // Reads the `ui.notificationDurationMs` setting via api.configuration
    // and registers its own configuration schema — configuration-service
    // must be loaded first.
    dependsOn: ['core.configuration-service'],
    contributes: {
      configuration: {
        pluginId: 'core.notification-service',
        title: 'Notifications',
        order: 20,
        category: 'system',
        schema: [
          {
            key: CONFIG_KEY_DURATION,
            title: 'Notification duration',
            description: 'Auto-dismiss duration for notifications in milliseconds',
            type: 'number' as const,
            default: DEFAULT_NOTIFICATION_DURATION_MS,
          },
        ],
      },
    },
  },

  activate(api: PluginAPI) {
    api.internal!.registerInternalService('notificationQueue', notificationQueue)
    api.configuration.register(notificationServicePlugin.manifest.contributes!.configuration!)
    // Paint the toast stack in the overlay slot. Low priority so toasts
    // never stack above blocking modals/banners sharing the slot; the
    // overlayFloating z-index keeps them above content, below modals.
    api.views.register(TOASTER_VIEW_ID, {
      slot: 'overlay',
      component: () =>
        createElement(NotificationToaster, {
          onAction: (command: string) => {
            void api.commands.execute(command)
          },
        }),
      priority: 5,
    })
    clientLogger.info('[core.notification-service] ready')
  },
}

// Re-exported for importers of the plugin module + tests.
export {
  NotificationQueue,
  notificationQueue,
  type Notification,
} from './notificationQueue'
