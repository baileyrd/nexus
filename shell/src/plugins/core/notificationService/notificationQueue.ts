// shell/src/plugins/core/notificationService/notificationQueue.ts
//
// The transient in-app notification (toast) queue. Split out of the
// plugin module so the producer (`api.notifications.show` →
// `notificationQueue.push`) and the overlay renderer
// (`NotificationToaster`, via `useSyncExternalStore`) can share one
// instance without a circular import through `index.ts`.

import { configStore } from '../../../stores/configStore'

export const DEFAULT_NOTIFICATION_DURATION_MS = 4000
export const CONFIG_KEY_DURATION = 'ui.notificationDurationMs'

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

/** Process-wide singleton. Registered as the `notificationQueue`
 *  internal service by the plugin's `activate`, and read directly by
 *  the `NotificationToaster` overlay renderer so both sides share state. */
export const notificationQueue = new NotificationQueue()
