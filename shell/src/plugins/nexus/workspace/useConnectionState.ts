// BL-140 Phase 3c — small hook that subscribes to
// `kernel:connection-state` Tauri events and returns the current
// remote-forge connection state. For local forges the bridge always
// reports `"idle"`, which the consumer should render as "no badge".

import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { clientLogger } from '../../../clientLogger'

export type ConnectionState =
  | 'idle'
  | 'connected'
  | 'reconnecting'
  | 'disconnected'

interface ConnectionStateEvent {
  state: ConnectionState
}

const CHANNEL = 'kernel:connection-state'

/**
 * Returns the latest [`ConnectionState`] from the kernel-runtime
 * managed-state slot. Subscribes to live transitions over the
 * `kernel:connection-state` Tauri event and falls back to the sync
 * `kernel_connection_state` read on mount so the badge has something
 * to render before the first transition fires.
 *
 * For local forges this always returns `'idle'` — the bridge only
 * tracks state for the remote variant.
 */
export function useConnectionState(): ConnectionState {
  const [state, setState] = useState<ConnectionState>('idle')

  useEffect(() => {
    let active = true

    // Initial snapshot.
    invoke<ConnectionState>('kernel_connection_state')
      .then((s) => {
        if (active) setState(s)
      })
      .catch((err) => {
        clientLogger.warn('[useConnectionState] initial read failed:', err)
      })

    // Live transitions.
    const unlistenPromise = listen<ConnectionStateEvent>(CHANNEL, (event) => {
      if (active) setState(event.payload.state)
    })

    return () => {
      active = false
      unlistenPromise.then((un) => un()).catch(() => undefined)
    }
  }, [])

  return state
}
