// src/plugins/core/notificationService/index.ts
// Service plugin — bootstraps the notification queue.
// After this activates, api.notifications.show() is available.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { configStore } from '../../../stores/configStore'
import { clientLogger } from '../../../clientLogger'

const DEFAULT_NOTIFICATION_DURATION_MS = 4000
const CONFIG_KEY_DURATION = 'ui.notificationDurationMs'

export interface Notification {
  id: string
  message: string
  type: 'info' | 'warning' | 'error' | 'success'
  duration: number
  actions?: Array<{ label: string; command: string }>
  timestamp: number
}

type NotificationListener = (notifications: Notification[]) => void

export class NotificationQueue {
  private items: Notification[] = []
  private listeners: NotificationListener[] = []
  private counter = 0

  push(n: Partial<Omit<Notification, 'id' | 'timestamp'>> & { message: string }) {
    const item: Notification = {
      type: 'info',
      duration: configStore.get<number>(CONFIG_KEY_DURATION, DEFAULT_NOTIFICATION_DURATION_MS) ?? DEFAULT_NOTIFICATION_DURATION_MS,
      ...n,
      id: `notif-${++this.counter}`,
      timestamp: Date.now(),
    }
    this.items = [...this.items, item]
    this.notify()

    if (item.duration > 0) {
      setTimeout(() => this.dismiss(item.id), item.duration)
    }
  }

  dismiss(id: string) {
    this.items = this.items.filter(i => i.id !== id)
    this.notify()
  }

  getAll() { return this.items }

  subscribe(fn: NotificationListener): () => void {
    this.listeners.push(fn)
    return () => { this.listeners = this.listeners.filter(l => l !== fn) }
  }

  private notify() {
    this.listeners.forEach(fn => fn(this.items))
  }
}

export const notificationServicePlugin: Plugin = {
  manifest: {
    id: 'core.notification-service',
    name: 'Notification Service',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    contributes: {
      configuration: {
        pluginId: 'core.notification-service',
        title: 'Notifications',
        order: 20,
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
    api.internal!.registerInternalService('notificationQueue', new NotificationQueue())
    api.configuration.register(notificationServicePlugin.manifest.contributes!.configuration!)
    clientLogger.info('[core.notification-service] ready')
  },
}
