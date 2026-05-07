// shell/src/plugins/nexus/terminal/HistoryView.tsx
//
// BL-060 — sub-view of nexus.terminal that lists recent ad-hoc command
// runs (the rows in `procmgr_adhoc_history`) with re-run, promote-to-
// saved, and delete affordances. Companion to SavedCommandsView; the
// layout intentionally mirrors that file so a user moving between the
// two panels has muscle-memory for the action row.
//
// Re-run sends the row's command literal through `send_input`, exactly
// as if the user had typed it. Promote opens an inline name field and
// then dispatches `adhoc_promote`; the new SavedCommand surfaces in the
// adjacent panel after the saved-commands store reloads.

import { useCallback, useEffect, useState } from 'react'
import type { KernelAPI, NotificationsAPI } from '../../../types/plugin'
import { useConfigValue } from '../../../stores/configStore'
import { Icon } from '../../../icons'
import { useTerminalStore } from './terminalStore'
import { useHistoryStore, type AdHocRecord, type AdHocStatus } from './historyStore'
import { useSavedCommandsStore } from './savedCommandsStore'

const PLUGIN_ID = 'com.nexus.terminal'
const CMD_SEND_INPUT = 'send_input'

const CMD_SAVE_NOTIFICATION_MS = 3000
const CMD_COPIED_NOTIFICATION_MS = 1800

interface HistoryViewProps {
  kernel: KernelAPI
  notifications: NotificationsAPI
  /** Called when the user wants to open / focus the terminal pane. */
  focusTerminal: () => void
}

/** Inline-promote state. `null` means no row is being promoted. */
type PromoteState = { id: string; name: string } | null

export function HistoryView(props: HistoryViewProps) {
  const { kernel, notifications, focusTerminal } = props
  const cmdSaveMs = useConfigValue('ui.commandSaveNotificationMs', CMD_SAVE_NOTIFICATION_MS)
  const cmdCopiedMs = useConfigValue(
    'ui.commandCopiedNotificationMs',
    CMD_COPIED_NOTIFICATION_MS,
  )

  const rows = useHistoryStore((s) => s.rows)
  const loaded = useHistoryStore((s) => s.loaded)
  const loading = useHistoryStore((s) => s.loading)
  const error = useHistoryStore((s) => s.error)
  const loadHistory = useHistoryStore((s) => s.loadHistory)
  const deleteHistory = useHistoryStore((s) => s.deleteHistory)
  const promoteHistory = useHistoryStore((s) => s.promoteHistory)
  const reloadSaved = useSavedCommandsStore((s) => s.loadSaved)

  const [promote, setPromote] = useState<PromoteState>(null)
  const [localError, setLocalError] = useState<string | null>(null)

  // Hydrate on first mount. Cheap when already loaded — the cache is in
  // memory.
  useEffect(() => {
    if (!loaded) void loadHistory(kernel)
  }, [loaded, loadHistory, kernel])

  const reload = useCallback(() => {
    setLocalError(null)
    void loadHistory(kernel)
  }, [loadHistory, kernel])

  const rerun = useCallback(
    async (row: AdHocRecord) => {
      setLocalError(null)
      const sessionId = useTerminalStore.getState().sessionId
      if (!sessionId) {
        focusTerminal()
        notifications.show({
          message: 'Opening terminal — click the command again to re-run it.',
          type: 'info',
          duration: cmdSaveMs ?? CMD_SAVE_NOTIFICATION_MS,
        })
        return
      }
      try {
        await kernel.invoke(PLUGIN_ID, CMD_SEND_INPUT, {
          id: sessionId,
          input: row.command,
        })
        focusTerminal()
        notifications.show({
          message: `Sent "${row.command}" to terminal`,
          type: 'success',
          duration: cmdCopiedMs ?? CMD_COPIED_NOTIFICATION_MS,
        })
      } catch (err) {
        setLocalError(String(err))
      }
    },
    [kernel, notifications, focusTerminal, cmdSaveMs, cmdCopiedMs],
  )

  const handleDelete = useCallback(
    async (id: string) => {
      setLocalError(null)
      try {
        await deleteHistory(kernel, id)
      } catch (err) {
        setLocalError(String(err))
      }
    },
    [deleteHistory, kernel],
  )

  const handlePromote = useCallback(async () => {
    if (!promote) return
    if (!promote.name.trim()) {
      setLocalError('Name is required to promote.')
      return
    }
    setLocalError(null)
    try {
      await promoteHistory(kernel, promote.id, promote.name.trim())
      setPromote(null)
      // Refresh the saved-commands cache so the new row shows up in
      // the adjacent panel without a manual reload.
      void reloadSaved(kernel)
      notifications.show({
        message: `Promoted to saved command "${promote.name}"`,
        type: 'success',
        duration: cmdCopiedMs ?? CMD_COPIED_NOTIFICATION_MS,
      })
    } catch (err) {
      setLocalError(String(err))
    }
  }, [promote, promoteHistory, kernel, reloadSaved, notifications, cmdCopiedMs])

  return (
    <div className="nexus-saved-commands">
      <header className="nexus-saved-commands-header">
        <h3>Command history</h3>
        <button
          type="button"
          className="nexus-saved-commands-add"
          onClick={reload}
          disabled={loading}
          title="Reload history"
        >
          <Icon name="refresh" size={12} />
        </button>
      </header>

      {(error || localError) && (
        <div className="nexus-saved-commands-error" role="alert">
          {localError ?? error}
        </div>
      )}

      {rows.length === 0 && !loading && (
        <p className="nexus-saved-commands-empty">
          No ad-hoc command history yet. Commands you run from the terminal
          will appear here.
        </p>
      )}

      <ul className="nexus-saved-commands-list">
        {rows.map((row) => (
          <li key={row.id} className="nexus-saved-command">
            <button
              type="button"
              className="nexus-saved-command-body"
              onClick={() => void rerun(row)}
              title="Re-run in active terminal"
            >
              <div className="nexus-saved-command-name">
                <code className="nexus-saved-command-cmd-inline">{row.command}</code>
              </div>
              <div className="nexus-saved-command-meta">
                <StatusChip status={row.status} />
                <span title="Run count">×{row.run_count}</span>
                <span title="Last run">{formatRelative(row.executed_at)}</span>
                {row.working_dir && <span>cwd: {row.working_dir}</span>}
              </div>
            </button>
            <div className="nexus-saved-command-actions">
              <button
                type="button"
                onClick={() => void rerun(row)}
                aria-label={`Re-run ${row.command}`}
                title="Re-run in active terminal"
              >
                <Icon name="play" size={12} />
              </button>
              <button
                type="button"
                onClick={() =>
                  setPromote({ id: row.id, name: defaultPromoteName(row.command) })
                }
                aria-label={`Promote ${row.command} to saved command`}
                title="Promote to saved command"
              >
                <Icon name="star" size={12} />
              </button>
              <button
                type="button"
                className="is-destructive"
                onClick={() => void handleDelete(row.id)}
                aria-label={`Forget ${row.command}`}
                title="Forget this row"
              >
                <Icon name="trash" size={12} />
              </button>
            </div>
            {promote?.id === row.id && (
              <PromoteForm
                name={promote.name}
                onChange={(name) => setPromote({ id: row.id, name })}
                onSubmit={() => void handlePromote()}
                onCancel={() => {
                  setLocalError(null)
                  setPromote(null)
                }}
              />
            )}
          </li>
        ))}
      </ul>
    </div>
  )
}

interface PromoteFormProps {
  name: string
  onChange: (name: string) => void
  onSubmit: () => void
  onCancel: () => void
}

function PromoteForm(props: PromoteFormProps) {
  const { name, onChange, onSubmit, onCancel } = props
  return (
    <form
      className="nexus-saved-command-form"
      onSubmit={(e) => {
        e.preventDefault()
        onSubmit()
      }}
    >
      <label>
        Saved-command name
        <input
          type="text"
          value={name}
          onChange={(e) => onChange(e.target.value)}
          autoFocus
        />
      </label>
      <div className="nexus-saved-command-form-actions">
        <button type="submit">Promote</button>
        <button type="button" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </form>
  )
}

function StatusChip({ status }: { status: AdHocStatus }) {
  // Mirrors the success / failure / timeout color cues used elsewhere
  // in the shell (git status, AI activity). No dedicated CSS class —
  // we lean on the meta typography and prefix the text with a glyph.
  const glyph =
    status === 'success' ? '✓' : status === 'failure' ? '✗' : '⏱'
  const color =
    status === 'success'
      ? 'var(--text-success, #38a169)'
      : status === 'failure'
        ? 'var(--text-error, #c53030)'
        : 'var(--text-faint)'
  return (
    <span title={`Last status: ${status}`} style={{ color }}>
      {glyph} {status}
    </span>
  )
}

/**
 * Pre-fill the promote form's name field from the command line.
 * Heuristic: take the first whitespace-delimited token (the program
 * name) and strip any path prefix. Empty fallback so the user just
 * types if the heuristic is wrong.
 */
function defaultPromoteName(command: string): string {
  const head = command.trim().split(/\s+/)[0] ?? ''
  const lastSlash = head.lastIndexOf('/')
  return lastSlash >= 0 ? head.slice(lastSlash + 1) : head
}

/**
 * Render a unix-second timestamp as a coarse relative string. Avoids
 * pulling in a date library for this single surface; the precision of
 * the bucket matches the History panel's "when did I roughly run this"
 * use case better than an absolute time would.
 */
function formatRelative(unixSec: number): string {
  const nowSec = Math.floor(Date.now() / 1000)
  const delta = Math.max(0, nowSec - unixSec)
  if (delta < 60) return `${delta}s ago`
  if (delta < 3600) return `${Math.floor(delta / 60)}m ago`
  if (delta < 86400) return `${Math.floor(delta / 3600)}h ago`
  if (delta < 86400 * 30) return `${Math.floor(delta / 86400)}d ago`
  // Beyond a month, fall back to absolute YYYY-MM-DD so the row's age
  // is unambiguous.
  return new Date(unixSec * 1000).toISOString().slice(0, 10)
}

