// BL-133 follow-up — subscribe to `com.nexus.notifications.delivered`
// and route each Desktop-channel notification through the existing
// `api.notifications.show` toast surface.
//
// The `nexus-notifications` core plugin (BL-133) publishes this
// event for every `Channel::Desktop` send. Without this subscriber
// the bus event fires with no observable effect; this plugin closes
// the loop.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { clientLogger } from '../../../clientLogger'
import { invoke } from '@tauri-apps/api/core'

/** Topic the `nexus-notifications` Desktop transport publishes on. */
const TOPIC = 'com.nexus.notifications.delivered'

/** Fire-and-forget OS-level notification via the `notify_desktop`
 *  bridge command (BL-133 follow-up). Failure is logged at debug — the
 *  in-app toast still surfaces the message, so a dev build without the
 *  Tauri bridge (running under Vite directly) keeps working. */
async function notifyDesktop(title: string, message: string): Promise<void> {
  try {
    await invoke('notify_desktop', { title, message })
  } catch (err) {
    clientLogger.debug('[nexus.notifications] notify_desktop unavailable:', err)
  }
}

/** Wire shape `DesktopTransport::send` emits — mirrors
 *  `crates/nexus-notifications/src/lib.rs::DesktopTransport`. */
export interface NotificationDeliveredPayload {
  channel: string
  title: string
  message: string
}

/** Decide which toast `type` chip to render for an incoming
 *  notification. The Rust side currently always emits `channel:
 *  "desktop"` for this topic, so we map by title prefix — a future
 *  enrichment (severity / source) can swap the projection rule
 *  without churning the wire shape. */
export function toastTypeFor(
  payload: NotificationDeliveredPayload,
): 'info' | 'warning' | 'error' | 'success' {
  const t = payload.title.toLowerCase()
  if (t.includes('error') || t.includes('failed')) return 'error'
  if (t.includes('warn') || t.includes('warning')) return 'warning'
  if (t.includes('done') || t.includes('complete') || t.includes('success'))
    return 'success'
  return 'info'
}

/** Compose the toast message body. We prepend the title (when it's
 *  not the default `"Nexus"` boilerplate) so the user can tell at a
 *  glance which subsystem emitted. */
export function composeToastMessage(payload: NotificationDeliveredPayload): string {
  const t = payload.title.trim()
  if (!t || t === 'Nexus') return payload.message
  return `${t}: ${payload.message}`
}

export const notificationsPlugin: Plugin = {
  manifest: {
    id: 'nexus.notifications',
    name: 'Notifications',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    contributes: {},
  },

  async activate(api: PluginAPI) {
    let unsub: (() => void) | null = null

    const subscribe = async () => {
      if (unsub) return
      try {
        unsub = await api.kernel.on<NotificationDeliveredPayload>(
          TOPIC,
          (_topic, payload) => {
            if (!payload || typeof payload.message !== 'string') return
            const message = composeToastMessage(payload)
            const type = toastTypeFor(payload)
            api.notifications.show({ message, type })
            // BL-133 follow-up — also fire the OS-level notification so
            // a backgrounded Nexus window still surfaces the alert.
            // The bridge is best-effort; the in-app toast above is the
            // authoritative surface.
            if (!document.hasFocus()) {
              const title = payload.title?.trim() || 'Nexus'
              void notifyDesktop(title, message)
            }
          },
        )
      } catch (err) {
        clientLogger.warn('[nexus.notifications] subscribe failed:', err)
        unsub = null
      }
    }

    const unsubscribe = () => {
      if (!unsub) return
      try {
        unsub()
      } catch (err) {
        clientLogger.warn('[nexus.notifications] unsubscribe failed:', err)
      }
      unsub = null
    }

    api.events.on('workspace:opened', () => {
      void subscribe()
    })
    api.events.on('workspace:closed', () => {
      unsubscribe()
    })

    if (await api.kernel.available()) {
      void subscribe()
    }
  },
}
