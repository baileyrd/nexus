// Forge inspector: Outline / Backlinks / Graph tabs.
// Outline is wired to the live doc store; Backlinks + Graph are
// visually complete placeholders pending real data sources.

import { useState } from 'react'
import { workspace } from '../../../workspace'
import { useDocStore } from '../../../stores/docStore'
import { Ic } from '../../../shell/icons'

type Tab = 'outline' | 'backlinks' | 'graph'

export function RightPanelView() {
  const [tab, setTab] = useState<Tab>('outline')
  const headings      = useDocStore(s => s.headings)
  const toggle        = () =>
    workspace.setSidedockCollapsed('right', !workspace.rightSplit.collapsed)

  return (
    <div className="rightpanel">
      <div className="panel-head">
        <span>Inspector</span>
        <div className="actions">
          <button className="icon-btn" title="Pin"><Ic.star /></button>
          <button className="icon-btn" title="Hide" onClick={toggle}><Ic.x /></button>
        </div>
      </div>

      <div className="rtabs">
        <div
          className={'rtab ' + (tab === 'outline' ? 'active' : '')}
          onClick={() => setTab('outline')}
        >
          Outline <span className="n">{headings.length}</span>
        </div>
        <div
          className={'rtab ' + (tab === 'backlinks' ? 'active' : '')}
          onClick={() => setTab('backlinks')}
        >
          Backlinks <span className="n">0</span>
        </div>
        <div
          className={'rtab ' + (tab === 'graph' ? 'active' : '')}
          onClick={() => setTab('graph')}
        >
          Graph
        </div>
      </div>

      <div className="rpanes">
        {tab === 'outline'   && <OutlinePane />}
        {tab === 'backlinks' && <BacklinksPane />}
        {tab === 'graph'     && <GraphPane />}
      </div>
    </div>
  )
}

function OutlinePane() {
  const headings      = useDocStore(s => s.headings)
  const active        = useDocStore(s => s.activeHeading)
  const jump          = useDocStore(s => s.jumpToHeading)

  return (
    <div className="rpane">
      <div className="ol-section">
        <span>Document outline</span>
        <span className="kbd" style={{ fontSize: 9 }}>{headings.length} hdrs</span>
      </div>
      {headings.length === 0 ? (
        <div style={{ color: 'var(--text-faint)', fontSize: 12, padding: '8px 6px' }}>
          No headings in the active document.
        </div>
      ) : (
        headings.map((h, idx) => (
          <div
            key={h.id}
            className={'ol-item lvl-' + h.level + (active === h.id ? ' active' : '')}
            onClick={() => jump(h.id)}
          >
            <span className="n">{String(idx + 1).padStart(2, '0')}</span>
            <span className="t">{h.text}</span>
          </div>
        ))
      )}
    </div>
  )
}

function BacklinksPane() {
  return (
    <div className="rpane">
      <div className="ol-section">
        <span>Linked mentions · 0</span>
        <span className="kbd" style={{ fontSize: 9 }}>0 in</span>
      </div>
      <div style={{ color: 'var(--text-faint)', fontSize: 12, padding: '8px 6px', lineHeight: 1.6 }}>
        Backlinks will appear here once an index is attached to the workspace.
      </div>
    </div>
  )
}

function GraphPane() {
  return (
    <div className="rpane">
      <div className="ol-section">
        <span>Local graph</span>
        <span className="kbd" style={{ fontSize: 9 }}>0 nodes</span>
      </div>
      <div style={{
        height: 220,
        display: 'grid', placeItems: 'center',
        color: 'var(--text-faint)', fontSize: 12,
        border: '1px dashed var(--divider-color)', borderRadius: 6,
        margin: 4,
      }}>
        Graph view — coming next
      </div>
    </div>
  )
}
