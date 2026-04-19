// src/shell/App.tsx
import { useEffect, useState } from 'react'
import { useSlotStore } from '../registry/SlotRegistry'
import { useLayoutStore } from '../stores/layoutStore'
import { usePaneModeStore } from '../stores/paneModeStore'
import { SlotSurface } from './slots/SlotSurface'
import { ResizeHandle } from './ResizeHandle'
import { getRegistry } from '../host/shellRegistry'
import { contextKeyService } from '../host/ContextKeyService'

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
        `titleBar: ${slots.titleBar.length}`,
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

      {/* Title bar */}
      <div className="shell-titlebar">
        <SlotSurface entries={slots.titleBar} />
      </div>

      {/* Body */}
      {(() => {
        // Pane-mode: one slot entry takes over the entire body region.
        // The activity bar stays visible so the user can switch out;
        // the titlebar, statusbar, and overlay are untouched (rendered
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
            <div className="shell-body">
              <div className="shell-activitybar">
                <SlotSurface entries={slots.activityBar} />
              </div>
              <div className="shell-pane-mode">
                <SlotSurface entries={[paneEntry]} />
              </div>
            </div>
          )
        }

        return (
          <div className="shell-body">
            <div className="shell-activitybar">
              <SlotSurface entries={slots.activityBar} />
            </div>

            {sidebar.visible && (
              <>
                <div className="shell-sidebar" style={{ width: sidebar.width }}>
                  <SlotSurface entries={slots.sidebar} />
                </div>
                <ResizeHandle direction="horizontal" onResize={resizeSidebar} />
              </>
            )}

            <div className="shell-center">
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
                <div className="shell-right-panel" style={{ width: rightPanel.width }}>
                  <SlotSurface entries={slots.rightPanel} />
                </div>
              </>
            )}
          </div>
        )
      })()}

      {/* Status bar */}
      <div className="shell-statusbar">
        <div className="shell-statusbar-left">
          <SlotSurface entries={slots.statusBarLeft} />
        </div>
        <div className="shell-statusbar-right">
          <SlotSurface entries={slots.statusBarRight} />
        </div>
      </div>

    </div>
  )
}
