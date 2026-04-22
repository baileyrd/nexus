// Phase-1 canvas surface: blank grey pane with a node/edge count
// overlay. Proves the `.canvas` → `canvas_read` IPC round-trip
// end-to-end. The real zoomable renderer lands in Phase 2.

import { useEffect } from 'react'
import { useCanvasStore } from './canvasStore'
import type { CanvasKernelClient } from './kernelClient'

interface Props {
  relpath: string
  client: CanvasKernelClient
}

export function CanvasView({ relpath, client }: Props) {
  const tab = useCanvasStore((s) => s.tabs.get(relpath))

  useEffect(() => {
    const store = useCanvasStore.getState()
    if (store.tabs.has(relpath)) return
    store.setLoading(relpath)
    void (async () => {
      try {
        const doc = await client.read(relpath)
        useCanvasStore.getState().setDoc(relpath, doc)
      } catch (err) {
        useCanvasStore.getState().setError(relpath, String(err))
      }
    })()
  }, [relpath, client])

  const doc = tab?.doc
  const nodeCount = doc?.nodes.length ?? 0
  const edgeCount = doc?.edges.length ?? 0

  return (
    <div
      style={{
        position: 'relative',
        width: '100%',
        height: '100%',
        background: 'var(--bg-muted, #1e1e1e)',
        overflow: 'hidden',
      }}
    >
      {tab?.loading && <CornerLabel>Loading…</CornerLabel>}
      {tab?.error && <CornerLabel>Error: {tab.error}</CornerLabel>}
      {!tab?.loading && !tab?.error && (
        <CornerLabel>
          {nodeCount} node{nodeCount === 1 ? '' : 's'} · {edgeCount} edge
          {edgeCount === 1 ? '' : 's'}
        </CornerLabel>
      )}
    </div>
  )
}

function CornerLabel({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        position: 'absolute',
        top: 8,
        right: 12,
        fontSize: 12,
        color: 'var(--fg-muted, #9ca3af)',
        fontFamily: 'var(--font-monospace, ui-monospace, monospace)',
        pointerEvents: 'none',
      }}
    >
      {children}
    </div>
  )
}
