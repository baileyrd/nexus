// src/shell/App.tsx
import { useEffect, useState } from 'react'
import { useSlotStore } from '../registry/SlotRegistry'
import { useLayoutStore } from '../stores/layoutStore'
import { usePaneModeStore } from '../stores/paneModeStore'
import { SlotSurface } from './slots/SlotSurface'
import { ResizeHandle } from './ResizeHandle'
import { getRegistry } from '../host/shellRegistry'
import { contextKeyService } from '../host/ContextKeyService'
import { useSidebarSplitStore } from '../plugins/nexus/sidebar/sidebarSplitStore'

export default function App() {
  const slots = useSlotStore(s => s.slots)
  const {
    sidebar, panelArea, rightPanel,
    resizeSidebar, resizePanelArea, resizeRightPanel,
  } = useLayoutStore()
  const paneModeViewId = usePaneModeStore(s => s.activeViewId)
  const [debugInfo, setDebugInfo] = useState<string>('')

  useEffect(() => {
    // Debug: log what's in each slot after mount
    const timer = setTimeout(() => {
      const reg = getRegistry()
      const info = [
        `Registry: ${reg ? 'loaded' : 'NULL'}`,
        `activityBar: ${slots.activityBar.length}`,
        `sidebar: ${slots.sidebar.length}`,
        `editorArea: ${slots.editorArea.length}`,
        `statusBarLeft: ${slots.statusBarLeft.length}`,
        `statusBarRight: ${slots.statusBarRight.length}`,
        `overlay: ${slots.overlay.length}`,
      ].join(' | ')
      console.info('[App] Slots:', info)
      setDebugInfo(info)
    }, 500)
    return () => clearTimeout(timer)
  }, [slots])

  // Boot-time view resolver — Obsidian-faithful. When sidebar.activeView
  // (or the panelArea equivalent) doesn't resolve against the live slot
  // registry, pick the highest-priority registered view and heal state
  // so activity-bar clicks, activity-bar state, and the rendered view
  // all agree. Runs whenever the relevant slot set changes, so plugin
  // enable/disable self-corrects too. The right panel has its own
  // first-registered-wins logic in rightPanelStore, so it's excluded.
  useEffect(() => {
    const sbEntries = slots.sidebarContent ?? []
    if (sbEntries.length === 0) return
    const current = useLayoutStore.getState().sidebar.activeView
    if (!current || !sbEntries.some((e) => e.id === current)) {
      // SlotRegistry already stores entries sorted ascending by priority.
      useLayoutStore.getState().setActiveSidebarView(sbEntries[0].id)
    }
    // Heal the split store too: if the user has no open leaves but a
    // sidebarContent view exists, seed one so the sidebar boots with a
    // visible tab (Obsidian's "first-registered-wins" behaviour). The
    // legacy activeView resolver above still runs for back-compat with
    // code that mirrors activeView for activity-bar highlighting.
    const split = useSidebarSplitStore.getState()
    if (split.leaves.length === 0) {
      split.revealLeaf(sbEntries[0].id)
    }
  }, [slots.sidebarContent])

  useEffect(() => {
    const paEntries = slots.panelArea ?? []
    if (paEntries.length === 0) return
    const current = useLayoutStore.getState().panelArea.activePanel
    if (current && paEntries.some((e) => e.id === current)) return
    useLayoutStore.getState().setActivePanel(paEntries[0].id)
  }, [slots.panelArea])

  // Global keyboard dispatcher
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement
      if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable) return
      const reg = getRegistry()
      if (!reg) return
      const keys = contextKeyService.snapshot()
      const commandId = reg.keybindings.match(e, keys)
      if (commandId) {
        e.preventDefault()
        e.stopPropagation()
        reg.commands.execute(commandId)
      }
    }
    document.addEventListener('keydown', handler)
    return () => document.removeEventListener('keydown', handler)
  }, [])

  const totalSlots = Object.values(slots).reduce((sum, arr) => sum + arr.length, 0)

  // If nothing is in any slot yet, show a loading indicator
  if (totalSlots === 0) {
    return (
      <div style={{
        height: '100vh',
        background: '#1e1e1e',
        color: '#cccccc',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        fontFamily: 'system-ui',
        fontSize: '13px',
      }}>
        Loading plugins...
      </div>
    )
  }

  return (
    <div className="shell-root">

      {/* Overlay */}
      <div className="shell-overlay">
        <SlotSurface entries={slots.overlay} />
      </div>

      {/* Workspace — Obsidian-faithful top-level container. Hosts the
          ribbon (.workspace-ribbon.mod-left) and the body columns
          (.workspace-split.mod-left-split / .mod-root / .mod-right-split)
          as direct flex siblings. */}
      <div className="workspace">

        {/* Activity bar — `.workspace-ribbon.mod-left` in Obsidian. */}
        <div className="workspace-ribbon mod-left">
          <SlotSurface entries={slots.activityBar} />
        </div>

        {(() => {
          // Pane-mode: one slot entry takes over the entire body region.
          // The activity bar stays visible (it's a sibling of this
          // branch); the statusbar and overlay are untouched (rendered
          // outside this branch).
          const paneEntry = paneModeViewId
            ? slots.paneMode.find(e => e.id === paneModeViewId)
            : undefined

          if (paneModeViewId && !paneEntry) {
            console.warn(
              `[App] Pane-mode viewId "${paneModeViewId}" is set but no matching slot entry exists; falling through to tri-pane.`,
            )
          }

          if (paneEntry) {
            return (
              <div className="shell-pane-mode">
                <SlotSurface entries={[paneEntry]} />
              </div>
            )
          }

          return (
            <>
              {sidebar.visible && (
                <>
                  <div className="workspace-split mod-left-split mod-vertical" style={{ width: sidebar.width }}>
                    <SlotSurface entries={slots.sidebar} />
                  </div>
                  <ResizeHandle direction="horizontal" onResize={resizeSidebar} />
                </>
              )}

              <div className="workspace-split mod-root mod-vertical">
                <div className="shell-editor-area">
                  <SlotSurface entries={slots.editorArea} />
                </div>

                {panelArea.visible && (
                  <>
                    <ResizeHandle direction="vertical" onResize={resizePanelArea} />
                    <div className="shell-panel-area" style={{ height: panelArea.height }}>
                      <SlotSurface entries={slots.panelArea} />
                    </div>
                  </>
                )}
              </div>

              {rightPanel.visible && (
                <>
                  <ResizeHandle direction="horizontal" onResize={resizeRightPanel} />
                  <div className="workspace-split mod-right-split mod-vertical" style={{ width: rightPanel.width }}>
                    <SlotSurface entries={slots.rightPanel} />
                  </div>
                </>
              )}
            </>
          )
        })()}
      </div>

      {/* Status bar — `.status-bar` matches Obsidian. Full-width at
          the bottom of shell-root; items on left/right in two segments. */}
      <div className="status-bar">
        <div className="status-bar-item-segment">
          <SlotSurface entries={slots.statusBarLeft} />
        </div>
        <div className="status-bar-item-segment">
          <SlotSurface entries={slots.statusBarRight} />
        </div>
      </div>

    </div>
  )
}
