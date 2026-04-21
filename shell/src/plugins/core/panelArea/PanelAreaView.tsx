import { usePanelAreaStore } from './panelAreaStore'
import { useSlotStore } from '../../../registry/SlotRegistry'
import { useLayoutStore } from '../../../stores/layoutStore'

export function PanelAreaView() {
  const { tabs, activeTabId, setActiveTab } = usePanelAreaStore()
  const entries = useSlotStore(s => s.slots.panelAreaContent)
  const togglePanel = useLayoutStore(s => s.togglePanelArea)

  const activeEntry = entries.find(e => e.id === activeTabId) ?? entries[0]

  return (
    <div className="panel-area">
      <div className="panel-area__tabbar">
        <div className="panel-area__tabs">
          {tabs.map(tab => (
            <button
              key={tab.id}
              className={`panel-tab ${activeTabId === tab.id ? 'panel-tab--active' : ''}`}
              onClick={() => setActiveTab(tab.id)}
            >
              {tab.title}
            </button>
          ))}
        </div>
        <button className="panel-area__close" onClick={togglePanel} title="Close Panel">✕</button>
      </div>
      <div className="panel-area__content">
        {activeEntry
          ? <activeEntry.component />
          : <div className="panel-area__empty">No panels registered</div>
        }
      </div>
    </div>
  )
}
