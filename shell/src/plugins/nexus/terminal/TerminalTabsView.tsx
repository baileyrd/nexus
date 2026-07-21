import { useEffect, useRef, useState } from 'react'
import type { KernelAPI, EventsAPI } from '../../../types/plugin'
import { useTerminalStore, formatBytesForChip } from './terminalStore'
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
  /**
   * Commit a manual tab rename: pins the title in the store and pushes
   * it to the kernel session label. Trimmed, non-empty names only — the
   * view drops blank input rather than calling this.
   */
  onRenameTab: (id: string, title: string) => void
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
  onRenameTab,
}: TerminalTabsViewProps) {
  const tabs = useTerminalStore((s) => s.tabs)
  const activeSessionId = useTerminalStore((s) => s.activeSessionId)
  const setActiveSession = useTerminalStore((s) => s.setActiveSession)
  const rssBytesBySession = useTerminalStore((s) => s.rssBytesBySession)

  // Inline-rename state: the id of the tab being edited (or null) plus
  // the working draft. Double-clicking a tab title enters edit mode;
  // Enter / blur commits, Escape cancels.
  const [editingId, setEditingId] = useState<string | null>(null)
  const [draft, setDraft] = useState('')
  const inputRef = useRef<HTMLInputElement | null>(null)

  // Focus + select the field when an edit begins so the user can type
  // over the old name immediately.
  useEffect(() => {
    if (editingId !== null) {
      inputRef.current?.focus()
      inputRef.current?.select()
    }
  }, [editingId])

  const beginEdit = (id: string, current: string) => {
    setEditingId(id)
    setDraft(current)
  }
  const cancelEdit = () => {
    setEditingId(null)
    setDraft('')
  }
  const commitEdit = (id: string) => {
    const next = draft.trim()
    if (next.length > 0) onRenameTab(id, next)
    cancelEdit()
  }

  return (
    <div className="nexus-terminal-tabs">
      <div className="nexus-terminal-tabbar" role="tablist">
        {tabs.map((tab) => {
          const isActive = tab.id === activeSessionId
          const isEditing = tab.id === editingId
          return (
            <div
              key={tab.id}
              role="tab"
              aria-selected={isActive}
              className={
                'nexus-terminal-tab' + (isActive ? ' is-active' : '')
              }
              onClick={() => setActiveSession(tab.id)}
              onDoubleClick={() => beginEdit(tab.id, tab.title)}
              title={isEditing ? undefined : `${tab.title} — double-click to rename`}
            >
              {isEditing ? (
                <input
                  ref={inputRef}
                  className="nexus-terminal-tab-rename"
                  value={draft}
                  onChange={(ev) => setDraft(ev.target.value)}
                  // The input lives inside the tab's click/dblclick
                  // handlers — stop propagation so typing / clicking in
                  // the field doesn't re-trigger select or re-enter edit.
                  onClick={(ev) => ev.stopPropagation()}
                  onDoubleClick={(ev) => ev.stopPropagation()}
                  onKeyDown={(ev) => {
                    if (ev.key === 'Enter') {
                      ev.preventDefault()
                      commitEdit(tab.id)
                    } else if (ev.key === 'Escape') {
                      ev.preventDefault()
                      cancelEdit()
                    }
                  }}
                  onBlur={() => commitEdit(tab.id)}
                />
              ) : (
                <span className="nexus-terminal-tab-title">{tab.title}</span>
              )}
              {/* #409 — memory chip: last known RSS from the periodic
                  list_sessions poll / a lifecycle event, whichever is
                  freshest. Absent until the first sample lands. */}
              {rssBytesBySession[tab.id] !== undefined ? (
                <span
                  className="nexus-terminal-tab-rss"
                  title="Approximate memory usage"
                >
                  {formatBytesForChip(rssBytesBySession[tab.id])}
                </span>
              ) : null}
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
