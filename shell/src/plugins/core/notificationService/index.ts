// src/plugins/core/notificationService/index.ts
// Service plugin — bootstraps the notification queue.
// After this activates, api.notifications.show() is available.

import type { Plugin, PluginAPI } from '../../../types/plugin'

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
      duration: 4000,
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
    contributes: {},
  },

  activate(api: PluginAPI) {
    api.internal!.registerInternalService('notificationQueue', new NotificationQueue())
    console.info('[core.notification-service] ready')
  },
}
