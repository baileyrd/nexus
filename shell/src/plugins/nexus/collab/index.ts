// BL-143 Phase 2.1 — `nexus.collab` shell plugin.
//
// Surfaces a peers panel populated by the Phase 1 collab wire events:
//
//   com.nexus.collab.peers.joined   — PeerInfo
//   com.nexus.collab.peers.left     — { peer_id }
//   com.nexus.collab.presence       — PresenceEvent
//   com.nexus.collab.connection     — { state }
//
// One prefix subscription on `com.nexus.collab.` covers all four; the
// handler routes by full topic and updates the Zustand store the panel
// renders from. PluginRegistry sweeps the disposer when the plugin
// unloads, so no manual teardown is needed here.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { workspace } from '../../../workspace'
import { collabPanelViewCreator } from './CollabPanelPaneView'
import { setCollabApi } from './collabRuntime'
import {
  useCollabStore,
  type ConnectionPayload,
  type PeerInfo,
  type PeerLeft,
  type PresenceEvent,
  type RelayStatus,
} from './collabStore'

const COLLAB_PLUGIN_ID = 'com.nexus.collab'
const TOPIC_PREFIX      = 'com.nexus.collab.'
const TOPIC_JOINED      = 'com.nexus.collab.peers.joined'
const TOPIC_LEFT        = 'com.nexus.collab.peers.left'
const TOPIC_PRESENCE    = 'com.nexus.collab.presence'
const TOPIC_CONN        = 'com.nexus.collab.connection'
const TOPIC_RELAY_START = 'com.nexus.collab.relay.started'
const TOPIC_RELAY_STOP  = 'com.nexus.collab.relay.stopped'

const VIEW_TYPE        = 'collab-panel'
const VIEW_ID          = 'nexus.collab.view'
const ACTIVITY_ITEM_ID = 'nexus.collab.activityItem'
const COMMAND_FOCUS    = 'nexus.collab.focus'

export const collabPlugin: Plugin = {
  manifest: {
    id: 'nexus.collab',
    name: 'Collaboration',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    popoutCompatible: false,
    dependsOn: ['nexus.workspace', 'nexus.activityBar'],
    contributes: {
      commands: [
        { id: COMMAND_FOCUS, title: 'Focus Collaboration Panel', category: 'Collaboration' },
      ],
    },
  },

  async activate(api: PluginAPI) {
    setCollabApi(api)
    api.viewRegistry.register(VIEW_TYPE, collabPanelViewCreator())

    // ── Bus subscriptions ─────────────────────────────────────────────
    // Single prefix subscription routes by full topic. The kernel may be
    // unavailable in popout / preview contexts; failures are non-fatal.
    if (await api.kernel.available()) {
      try {
        await api.kernel.on<unknown>(TOPIC_PREFIX, (topic, payload) => {
          const store = useCollabStore.getState()
          switch (topic) {
            case TOPIC_JOINED:
              store.onPeerJoined(payload as PeerInfo)
              return
            case TOPIC_LEFT:
              store.onPeerLeft(payload as PeerLeft)
              return
            case TOPIC_PRESENCE:
              store.onPresence(payload as PresenceEvent)
              return
            case TOPIC_CONN:
              store.onConnection(payload as ConnectionPayload)
              return
            case TOPIC_RELAY_START:
            case TOPIC_RELAY_STOP:
              // BL-143 Phase 2.3 — relay-host state changes flow
              // through the bus so popout windows + the main shell
              // stay synced without polling.
              store.onRelayStatus(payload as RelayStatus)
              return
            default:
              // Future BL-143 topics under com.nexus.collab.* land here.
          }
        })

        // Hydrate the relay slice at activation — the backend may
        // already have a relay running (e.g. shell reload) and we
        // don't want the Share button stuck on "Share this forge"
        // when one is live.
        try {
          const status = await api.kernel.invoke<RelayStatus>(
            COLLAB_PLUGIN_ID,
            'relay_status',
            {},
          )
          useCollabStore.getState().onRelayStatus(status)
        } catch {
          // No status = panel just doesn't show a Stop button.
        }
      } catch {
        // No subscription = panel stays in 'idle'; the user sees the
        // "Not configured" empty state. Still better than crashing.
      }
    }

    api.events.on('workspace:closed', () => useCollabStore.getState().reset())

    // ── Focus command + activity-bar entry ────────────────────────────
    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'left')
      workspace.revealLeaf(leaf)
    })

    api.activityBar.addItem({
      id: ACTIVITY_ITEM_ID,
      icon: '',
      iconName: 'users',
      title: 'Collaboration',
      viewId: VIEW_ID,
      priority: 27,
      command: COMMAND_FOCUS,
    })
  },
}
