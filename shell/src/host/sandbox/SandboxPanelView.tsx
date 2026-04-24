// shell/src/host/sandbox/SandboxPanelView.tsx
//
// WI-30d — React renderer for PanelNode trees returned by sandboxed
// community plugins. Per docs/wi30-sandbox-design.md §6.
//
// Plugins running in the iframe cannot ship React components across
// `postMessage` — closures and refs do not survive structured-clone.
// Instead they call `ctx.views.registerPanel(viewId, () => PanelNode)`
// and the host-side orchestrator requests fresh trees via an RPC
// round-trip. This component is that host-side consumer.
//
// Rendering contract:
//   - Walks the PanelNode tree using a switch on `node.type`.
//   - Primitives are the approved set from `@nexus/extension-api`:
//     vstack, hstack, text, heading, button, spacer. No HTML escape
//     hatch, no `dangerouslySetInnerHTML`, no arbitrary tag names.
//   - Button clicks dispatch via `api.commands.execute(commandId)`.
//     Sandboxed commands round-trip through the orchestrator bridge
//     installed in SandboxOrchestrator; first-party commands use the
//     normal registry path. Both surfaces share `api.commands.execute`.
//
// Refresh path:
//   - Initial mount: call `orchestrator.renderPanel(renderSub)` once.
//   - On a guest-emitted `views.requestRefresh` event: re-fetch.
//   - The component exposes a public `refresh()` helper via ref only
//     if a caller opts into that; by default the refresh loop is
//     driven entirely by events.

import { useCallback, useEffect, useState } from 'react'
import type { PanelNode } from '@nexus/extension-api'
import type { PluginAPI } from '../../types/plugin'
import type { SandboxInstance } from './SandboxOrchestrator'

export interface SandboxPanelViewProps {
  /**
   * The orchestrator-managed instance hosting the plugin. When the
   * instance's state flips to `crashed` or `disposed`, the component
   * displays a muted placeholder instead of stale content.
   */
  instance: SandboxInstance
  /**
   * The renderSub the guest registered for this panel. Passed to
   * `instance.renderPanel(renderSub)`.
   */
  renderSub: string
  /**
   * Host PluginAPI — used for command execution on button clicks.
   * The caller threads its own API through; this component does not
   * reach into a global singleton.
   */
  api: Pick<PluginAPI, 'commands'>
}

export function SandboxPanelView(props: SandboxPanelViewProps): JSX.Element {
  const { instance, renderSub, api } = props
  const [node, setNode] = useState<PanelNode | null>(null)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async (): Promise<void> => {
    try {
      const next = await instance.renderPanel(renderSub)
      setNode(next)
      setError(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
      setNode(null)
    }
  }, [instance, renderSub])

  useEffect(() => {
    void refresh()
    // NB: SandboxInstance does not (yet) expose a refresh event stream.
    // When the guest emits `views.requestRefresh` the orchestrator can
    // forward into a subscription exposed on the instance; wire that
    // up when the runtime gains the channel. For now the initial
    // render is the only automatic one; callers can trigger additional
    // refreshes by unmounting/remounting the component.
    return () => {
      // no-op: nothing to clean up on the React side
    }
  }, [refresh])

  if (instance.state === 'crashed') {
    return (
      <div className="sandbox-panel sandbox-panel--crashed" role="alert">
        Plugin "{instance.pluginId}" has crashed. Click to restart.
      </div>
    )
  }
  if (instance.state === 'disposed') {
    return (
      <div className="sandbox-panel sandbox-panel--disposed" aria-hidden>
        (plugin unloaded)
      </div>
    )
  }
  if (error) {
    return (
      <div className="sandbox-panel sandbox-panel--error" role="alert">
        Panel render failed: {error}
      </div>
    )
  }
  if (!node) {
    return <div className="sandbox-panel sandbox-panel--loading" aria-busy />
  }

  return (
    <div className="sandbox-panel" data-plugin-id={instance.pluginId}>
      {renderPanelNode(node, api)}
    </div>
  )
}

// ─── Primitive dispatcher ───────────────────────────────────────────────────

/**
 * Walk a PanelNode tree and emit React elements. Exported for tests
 * so the renderer can be exercised without mounting SandboxPanelView.
 */
export function renderPanelNode(
  node: PanelNode,
  api: Pick<PluginAPI, 'commands'>,
  keyPrefix = 'pn',
): JSX.Element {
  switch (node.type) {
    case 'vstack':
      return (
        <div
          key={keyPrefix}
          className="panel-vstack"
          style={{
            display: 'flex',
            flexDirection: 'column',
            gap: node.gap ?? 8,
          }}
        >
          {node.children.map((child, i) =>
            renderPanelNode(child, api, `${keyPrefix}.${i}`),
          )}
        </div>
      )
    case 'hstack':
      return (
        <div
          key={keyPrefix}
          className="panel-hstack"
          style={{
            display: 'flex',
            flexDirection: 'row',
            gap: node.gap ?? 8,
          }}
        >
          {node.children.map((child, i) =>
            renderPanelNode(child, api, `${keyPrefix}.${i}`),
          )}
        </div>
      )
    case 'text':
      return (
        <span
          key={keyPrefix}
          className={
            'panel-text' +
            (node.muted ? ' panel-text--muted' : '') +
            (node.strong ? ' panel-text--strong' : '')
          }
        >
          {node.value}
        </span>
      )
    case 'heading': {
      const level = node.level ?? 2
      // Use a tag lookup instead of dynamic JSX so TS stays happy
      // with the constrained set of heading elements.
      if (level === 1) return <h1 key={keyPrefix}>{node.value}</h1>
      if (level === 3) return <h3 key={keyPrefix}>{node.value}</h3>
      return <h2 key={keyPrefix}>{node.value}</h2>
    }
    case 'button':
      return (
        <button
          key={keyPrefix}
          type="button"
          className="panel-button"
          disabled={node.disabled}
          onClick={() => {
            // Fire-and-forget — return value surfaces via the plugin's
            // own event bus if the plugin wants to observe it.
            // Promise rejection is swallowed; the plugin can surface
            // its own error via notifications.show.
            void Promise.resolve(api.commands.execute(node.commandId)).catch(
              (err) => {
                console.warn(
                  '[SandboxPanelView] command execute threw',
                  node.commandId,
                  err,
                )
              },
            )
          }}
        >
          {node.label}
        </button>
      )
    case 'spacer':
      return (
        <div
          key={keyPrefix}
          className="panel-spacer"
          style={{ height: node.size ?? 8 }}
          aria-hidden
        />
      )
  }
}
