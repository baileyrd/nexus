// BL-141 follow-up — diagnostics panel plugin.
//
// Closes the third Phase-3 consumer in BL-141: subscribes globally to
// `com.nexus.lsp.textDocument.publishDiagnostics`, aggregates the
// latest set per URI in `diagnosticsStore`, and renders the panel
// (file-grouped list + per-row click-to-jump + "Open all in
// multibuffer" toolbar button). The multibuffer button funnels every
// in-forge diagnostic through the BL-141 `open_excerpts` path via the
// `diagnosticsToExcerptRequests` converter and emits `files:open` with
// the synthetic `multibuffer://` relpath.

import { createElement } from 'react'

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { useWorkspaceStore } from '../workspace/workspaceStore'
import { usePaneModeStore } from '../../../stores/paneModeStore'
import { clientLogger } from '../../../clientLogger'
import { EditorKernelClient } from '../editor/kernelClient'
import {
  LSP_DIAGNOSTICS_TOPIC,
  type LspDiagnostic,
  type PublishDiagnosticsParams,
} from '../editor/cm/lspIpc'
import { diagnosticsToExcerptRequests } from '../editor/cm/lspToExcerpts'
import { DiagnosticsPanelView } from './DiagnosticsPanelView'
import { useDiagnosticsStore } from './diagnosticsStore'

const PLUGIN_ID = 'nexus.diagnostics'
const VIEW_ID = 'nexus.diagnostics.view'
const ACTIVITY_ITEM_ID = 'nexus.diagnostics.activityItem'
const COMMAND_SHOW = 'nexus.diagnostics.show'
const COMMAND_OPEN_IN_MULTIBUFFER = 'nexus.diagnostics.openInMultibuffer'

const COMMAND_PANE_MODE_ENTER = 'nexus.paneMode.enter'
const COMMAND_PANE_MODE_EXIT = 'nexus.paneMode.exit'

const EVENT_ACTIVITY_BAR_ACTIVE_CHANGED = 'activityBar:activeChanged'
const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

const EVENT_FILES_OPEN = 'files:open'
const EVENT_EDITOR_REVEAL_LINE = 'nexus.editor:reveal-line'

/** Lucide `circle-alert` — stroke-only, matches the iconPath contract
 *  used by the other activity-bar items. */
const ALERT_ICON_PATH =
  'M12 8v4 M12 16h.01 M22 12c0 5.523-4.477 10-10 10S2 17.523 2 12 6.477 2 12 2s10 4.477 10 10z'

/** Validate the bus payload shape before mutating the store. The bus
 *  is untyped at the boundary; a bad payload could blow up the entire
 *  panel render. */
function isPublishDiagnosticsParams(
  payload: unknown,
): payload is PublishDiagnosticsParams {
  if (!payload || typeof payload !== 'object') return false
  const p = payload as Record<string, unknown>
  if (typeof p.uri !== 'string') return false
  if (!Array.isArray(p.diagnostics)) return false
  return true
}

/** Pulled out for the multibuffer command — the store keys by URI so
 *  the converter's URI-keyed input shape is the natural fit. */
function snapshotByUri(): Record<string, LspDiagnostic[]> {
  const out: Record<string, LspDiagnostic[]> = {}
  for (const [uri, diags] of useDiagnosticsStore.getState().byUri) {
    out[uri] = diags
  }
  return out
}

export const diagnosticsPlugin: Plugin = {
  manifest: {
    id: PLUGIN_ID,
    name: 'Diagnostics',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    // Imports `../workspace/workspaceStore` and `../editor/kernelClient`
    // + `../editor/cm/lspIpc` + `../editor/cm/lspToExcerpts`, so the
    // workspace and editor plugins must be loaded first.
    dependsOn: ['nexus.paneMode', 'nexus.activityBar', 'nexus.workspace', 'nexus.editor'],
    contributes: {
      commands: [
        {
          id: COMMAND_SHOW,
          title: 'Show Diagnostics',
          category: 'Diagnostics',
        },
        {
          id: COMMAND_OPEN_IN_MULTIBUFFER,
          title: 'Open All Diagnostics in Multibuffer',
          category: 'Diagnostics',
        },
      ],
    },
  },

  async activate(api: PluginAPI) {
    const editorClient = new EditorKernelClient(api.kernel)

    /** Open a single diagnostic — raise the file tab + reveal the
     *  diagnostic's start position. The reveal handler is the same
     *  one Cmd+Click → definition uses (`nexus.editor:reveal-line`),
     *  so tab-load + scroll race conditions are handled by the
     *  receiving plugin. */
    const openDiagnostic = (uri: string, diag: LspDiagnostic): void => {
      const forgeRoot = useWorkspaceStore.getState().rootPath
      if (!forgeRoot) return
      // Reuse `diagnosticsToExcerptRequests`'s URI conversion via a
      // single-entry map — it returns [] for out-of-forge URIs so we
      // short-circuit cleanly without re-implementing the prefix
      // logic here.
      const probe = diagnosticsToExcerptRequests(
        { [uri]: [diag] },
        { forgeRoot },
      )
      if (probe.length === 0) return
      const relpath = probe[0].relpath
      const lastSlash = Math.max(
        relpath.lastIndexOf('/'),
        relpath.lastIndexOf('\\'),
      )
      const name = lastSlash >= 0 ? relpath.slice(lastSlash + 1) : relpath
      api.events.emit(EVENT_FILES_OPEN, { relpath, name })
      api.events.emit(EVENT_EDITOR_REVEAL_LINE, {
        relpath,
        line: diag.range?.start?.line ?? 0,
        character: diag.range?.start?.character ?? 0,
      })
    }

    /** Funnel every in-forge diagnostic through `open_excerpts` and
     *  emit `files:open` with the synthetic relpath. Mirrors the
     *  shape of `COMMAND_LSP_FIND_REFERENCES` /
     *  `COMMAND_LSP_RENAME_PREVIEW`. */
    const openAllInMultibuffer = async (): Promise<void> => {
      const forgeRoot = useWorkspaceStore.getState().rootPath
      if (!forgeRoot) {
        api.notifications.show({
          type: 'error',
          message: 'Open diagnostics failed: no workspace open.',
        })
        return
      }
      const byUri = snapshotByUri()
      const items = diagnosticsToExcerptRequests(byUri, { forgeRoot })
      if (items.length === 0) {
        api.notifications.show({
          type: 'info',
          message: 'No diagnostics in the forge to open.',
        })
        return
      }
      try {
        const snap = await editorClient.openExcerpts(items)
        api.events.emit(EVENT_FILES_OPEN, {
          relpath: snap.relpath,
          name: `Diagnostics (${items.length})`,
        })
      } catch (err) {
        api.notifications.show({
          type: 'error',
          message: `Open diagnostics view failed: ${
            err instanceof Error ? err.message : String(err)
          }`,
        })
      }
    }

    // ── View registration ─────────────────────────────────────────────
    api.views.register(VIEW_ID, {
      slot: 'paneMode',
      component: () =>
        createElement(DiagnosticsPanelView, {
          forgeRoot: useWorkspaceStore.getState().rootPath,
          onOpenInMultibuffer: () => {
            void openAllInMultibuffer()
          },
          onOpenDiagnostic: openDiagnostic,
        }),
      priority: 14,
    })

    // ── Activity-bar item ─────────────────────────────────────────────
    api.activityBar.addItem({
      id: ACTIVITY_ITEM_ID,
      icon: '',
      iconPath: ALERT_ICON_PATH,
      title: 'Diagnostics',
      viewId: VIEW_ID,
      priority: 56,
    })

    // ── Activity-bar routing ──────────────────────────────────────────
    api.events.on<{ viewId: string | null }>(
      EVENT_ACTIVITY_BAR_ACTIVE_CHANGED,
      ({ viewId }) => {
        if (viewId === VIEW_ID) {
          void api.commands.execute(COMMAND_PANE_MODE_ENTER, VIEW_ID)
        } else {
          const current = usePaneModeStore.getState().activeViewId
          if (current === VIEW_ID) {
            void api.commands.execute(COMMAND_PANE_MODE_EXIT)
          }
        }
      },
    )

    // ── Commands ──────────────────────────────────────────────────────
    api.commands.register(COMMAND_SHOW, async () => {
      await api.commands.execute(COMMAND_PANE_MODE_ENTER, VIEW_ID)
    })
    api.commands.register(COMMAND_OPEN_IN_MULTIBUFFER, async () => {
      await openAllInMultibuffer()
    })

    // ── Bus subscription — accumulate diagnostics globally ────────────
    let unsub: (() => void) | null = null

    const subscribe = async () => {
      if (unsub) return
      try {
        unsub = await api.kernel.on<unknown>(
          LSP_DIAGNOSTICS_TOPIC,
          (_topic, payload) => {
            if (!isPublishDiagnosticsParams(payload)) return
            useDiagnosticsStore
              .getState()
              .setForUri(payload.uri, payload.diagnostics)
          },
        )
      } catch (err) {
        clientLogger.warn('[nexus.diagnostics] subscribe failed:', err)
        unsub = null
      }
    }

    const unsubscribe = () => {
      if (!unsub) return
      try {
        unsub()
      } catch (err) {
        clientLogger.warn('[nexus.diagnostics] unsubscribe failed:', err)
      }
      unsub = null
    }

    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      useDiagnosticsStore.getState().clear()
      void subscribe()
    })
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      unsubscribe()
      useDiagnosticsStore.getState().clear()
    })

    // Cover the boot race: workspace:opened may have fired before our
    // listener attached.
    if (await api.kernel.available()) {
      void subscribe()
    }
  },
}
