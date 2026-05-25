import type { KernelAPI, EventsAPI } from '../../../types/plugin'
import { useTerminalStore } from './terminalStore'
import { TerminalInstance } from './TerminalInstance'
import './terminal.css'

interface TerminalTabsViewProps {
  kernel: KernelAPI
  events: EventsAPI
  openExternal: (target: string) => Promise<void>
  /** Spawn a new session + tab and make it active. */
  onNewTab: () => void
  /** Close the session backing `id` and drop its tab. */
  onCloseTab: (id: string) => void
}

/**
 * Zed-style terminal panel root: a tab strip across the top plus one
 * live xterm per tab beneath it. All instances stay mounted so each
 * terminal keeps its own scrollback / PTY state; only the active one is
 * visible. The leaf hosts exactly one of these for the whole panel.
 */
export function TerminalTabsView({
  kernel,
  events,
  openExternal,
  onNewTab,
  onCloseTab,
}: TerminalTabsViewProps) {
  const tabs = useTerminalStore((s) => s.tabs)
  const activeSessionId = useTerminalStore((s) => s.activeSessionId)
  const setActiveSession = useTerminalStore((s) => s.setActiveSession)

  return (
    <div className="nexus-terminal-tabs">
      <div className="nexus-terminal-tabbar" role="tablist">
        {tabs.map((tab) => {
          const isActive = tab.id === activeSessionId
          return (
            <div
              key={tab.id}
              role="tab"
              aria-selected={isActive}
              className={
                'nexus-terminal-tab' + (isActive ? ' is-active' : '')
              }
              onClick={() => setActiveSession(tab.id)}
              title={tab.title}
            >
              <span className="nexus-terminal-tab-title">{tab.title}</span>
              <button
                type="button"
                className="nexus-terminal-tab-close"
                aria-label={`Close ${tab.title}`}
                title="Close terminal"
                onClick={(ev) => {
                  // Don't let the click bubble to the tab's
                  // select-on-click handler.
                  ev.stopPropagation()
                  onCloseTab(tab.id)
                }}
              >
                {'×'}
              </button>
            </div>
          )
        })}
        <button
          type="button"
          className="nexus-terminal-tab-new"
          aria-label="New terminal"
          title="New terminal"
          onClick={onNewTab}
        >
          {'+'}
        </button>
      </div>
      <div className="nexus-terminal-tabs-body">
        {tabs.map((tab) => (
          <TerminalInstance
            key={tab.id}
            sessionId={tab.id}
            active={tab.id === activeSessionId}
            kernel={kernel}
            events={events}
            openExternal={openExternal}
          />
        ))}
        {tabs.length === 0 && (
          <div className="nexus-terminal-empty">
            <button
              type="button"
              className="nexus-terminal-empty-new"
              onClick={onNewTab}
            >
              New terminal
            </button>
          </div>
        )}
      </div>
    </div>
  )
}
