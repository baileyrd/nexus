// shell/src/plugins/nexus/terminal/SavedCommandsView.tsx
//
// CommandBook-style sidebar: each saved command is a persistent named
// process. Clicking a running entry switches the terminal pane to its
// session output. Clicking a stopped entry spawns a dedicated session
// for that command and reveals the terminal pane.
//
// Layout:
//   ┌─ header ──────────────────────────────────────┐
//   │  Processes              [+ New] [⊕ Terminal]  │
//   ├───────────────────────────────────────────────┤
//   │  ● Dev server                  [Stop]   […]   │
//   │    npm run dev                               │
//   │  ○ Tests                       [Run]    […]   │
//   │    cargo test                               │
//   └───────────────────────────────────────────────┘
//
// ● green = active session exists  ○ gray = no session

import { useCallback, useEffect, useMemo, useState } from 'react'
import type { KernelAPI, NotificationsAPI } from '../../../types/plugin'
import { useConfigValue } from '../../../stores/configStore'
import { useTerminalStore, type SessionEntry } from './terminalStore'
import { useWorkspaceStore } from '../workspace/workspaceStore'
import {
  useSavedCommandsStore,
  type SavedCommand,
  type SavedCommandDraft,
} from './savedCommandsStore'

const CMD_SAVE_NOTIFICATION_MS = 3000

const PLUGIN_ID = 'com.nexus.terminal'
const HANDLER_CREATE_SESSION = 'create_session'
const HANDLER_CLOSE_SESSION = 'close_session'

interface CreateSessionResponse {
  id: string
}

interface SavedCommandsViewProps {
  kernel: KernelAPI
  notifications: NotificationsAPI
  /** Reveals the terminal pane and focuses xterm. */
  focusTerminal: () => void
  /** Creates a new ad-hoc interactive terminal and reveals the pane. */
  onNewTerminal: () => Promise<void>
}

const EMPTY_DRAFT: SavedCommandDraft = {
  slug: '',
  name: '',
  shell: '',
  shell_cmd: '',
  working_dir: null,
  icon: 'terminal',
}

type EditorState =
  | { mode: 'closed' }
  | { mode: 'add'; draft: SavedCommandDraft }
  | { mode: 'edit'; original: SavedCommand; draft: SavedCommandDraft }

function slugify(name: string): string {
  const base = name
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
  return base || `cmd-${Date.now().toString(36)}`
}

export function SavedCommandsView(props: SavedCommandsViewProps) {
  const { kernel, notifications, focusTerminal, onNewTerminal } = props
  const cmdSaveMs = useConfigValue('ui.commandSaveNotificationMs', CMD_SAVE_NOTIFICATION_MS)

  const commands = useSavedCommandsStore((s) => s.commands)
  const loaded = useSavedCommandsStore((s) => s.loaded)
  const error = useSavedCommandsStore((s) => s.error)
  const loadSaved = useSavedCommandsStore((s) => s.loadSaved)
  const createSaved = useSavedCommandsStore((s) => s.createSaved)
  const updateSaved = useSavedCommandsStore((s) => s.updateSaved)
  const deleteSaved = useSavedCommandsStore((s) => s.deleteSaved)
  const reorderSaved = useSavedCommandsStore((s) => s.reorderSaved)

  // Read multi-session state from the terminal store.
  const slugSessions = useTerminalStore((s) => s.slugSessions)
  const activeSessionId = useTerminalStore((s) => s.activeSessionId)

  const [editor, setEditor] = useState<EditorState>({ mode: 'closed' })
  const [localError, setLocalError] = useState<string | null>(null)
  const [starting, setStarting] = useState<Set<string>>(new Set())

  useEffect(() => {
    if (!loaded) void loadSaved(kernel)
  }, [loaded, loadSaved, kernel])

  const slugSet = useMemo(() => new Set(commands.map((c) => c.slug)), [commands])

  const pickSlug = useCallback(
    (name: string): string => {
      const base = slugify(name)
      if (!slugSet.has(base)) return base
      for (let i = 2; i < 1000; i += 1) {
        const candidate = `${base}-${i}`
        if (!slugSet.has(candidate)) return candidate
      }
      return `${base}-${Date.now().toString(36)}`
    },
    [slugSet],
  )

  /**
   * Start a dedicated session for `cmd`.
   *
   * Flow: create_session → send_input (the command text) → addSession →
   * setActiveSession → focusTerminal. The shell opens at the command's
   * working_dir if set; the shell binary uses the saved command's `shell`
   * field (falling back to platform default when empty).
   */
  const runCommand = useCallback(
    async (cmd: SavedCommand) => {
      // If a session already exists for this slug, just switch to it.
      const existingId = useTerminalStore.getState().slugSessions[cmd.slug]
      if (existingId) {
        useTerminalStore.getState().setActiveSession(existingId)
        focusTerminal()
        return
      }

      setLocalError(null)
      setStarting((prev) => new Set(prev).add(cmd.slug))

      try {
        const workspaceRoot = useWorkspaceStore.getState().rootPath
        const resp = await kernel.invoke<CreateSessionResponse>(
          PLUGIN_ID,
          HANDLER_CREATE_SESSION,
          {
            name: cmd.name,
            shell: cmd.shell || undefined,
            working_dir: cmd.working_dir ?? workspaceRoot ?? undefined,
          },
        )

        const entry: SessionEntry = { name: cmd.name, savedCommandSlug: cmd.slug }
        useTerminalStore.getState().addSession(resp.id, entry)
        useTerminalStore.getState().setActiveSession(resp.id)

        // Send the command text so the shell runs it immediately.
        await kernel.invoke(PLUGIN_ID, 'send_input', {
          id: resp.id,
          input: cmd.shell_cmd,
        })

        focusTerminal()
      } catch (err) {
        setLocalError(String(err))
      } finally {
        setStarting((prev) => {
          const next = new Set(prev)
          next.delete(cmd.slug)
          return next
        })
      }
    },
    [kernel, focusTerminal],
  )

  /** Close the session associated with `slug`. */
  const stopCommand = useCallback(
    async (slug: string) => {
      const id = useTerminalStore.getState().slugSessions[slug]
      if (!id) return
      setLocalError(null)
      try {
        useTerminalStore.getState().removeSession(id)
        await kernel.invoke(PLUGIN_ID, HANDLER_CLOSE_SESSION, { id })
      } catch (err) {
        setLocalError(String(err))
      }
    },
    [kernel],
  )

  const handleSubmit = useCallback(async () => {
    if (editor.mode === 'closed') return
    const draft = editor.draft
    if (!draft.name.trim() || !draft.shell_cmd.trim()) {
      setLocalError('Name and command are required.')
      return
    }
    setLocalError(null)
    try {
      if (editor.mode === 'add') {
        const slug = pickSlug(draft.name)
        await createSaved(kernel, { ...draft, slug })
      } else {
        await updateSaved(kernel, { ...draft, slug: editor.original.slug })
      }
      setEditor({ mode: 'closed' })
    } catch (err) {
      setLocalError(String(err))
    }
  }, [editor, createSaved, updateSaved, kernel, pickSlug])

  const handleDelete = useCallback(
    async (slug: string) => {
      // Stop the session first so there's no orphan.
      await stopCommand(slug)
      setLocalError(null)
      try {
        await deleteSaved(kernel, slug)
      } catch (err) {
        setLocalError(String(err))
      }
    },
    [deleteSaved, kernel, stopCommand],
  )

  const handleMove = useCallback(
    async (idx: number, dir: -1 | 1) => {
      const target = idx + dir
      if (target < 0 || target >= commands.length) return
      const next = commands.slice()
      const [row] = next.splice(idx, 1)
      next.splice(target, 0, row)
      setLocalError(null)
      try {
        await reorderSaved(kernel, next.map((c) => c.slug))
      } catch (err) {
        setLocalError(String(err))
      }
    },
    [commands, reorderSaved, kernel],
  )

  return (
    <div className="nexus-saved-commands">
      <header className="nexus-saved-commands-header">
        <h3>Processes</h3>
        <div className="nexus-saved-commands-header-actions">
          <button
            type="button"
            className="nexus-saved-commands-add"
            onClick={() => setEditor({ mode: 'add', draft: { ...EMPTY_DRAFT } })}
            title="Add saved command"
          >
            + New
          </button>
          <button
            type="button"
            className="nexus-saved-commands-new-terminal"
            onClick={() => void onNewTerminal()}
            title="Open a new interactive terminal"
          >
            ⊕
          </button>
        </div>
      </header>

      {(error || localError) && (
        <div className="nexus-saved-commands-error" role="alert">
          {localError ?? error}
        </div>
      )}

      {editor.mode !== 'closed' && (
        <CommandForm
          draft={editor.draft}
          submitLabel={editor.mode === 'add' ? 'Add' : 'Save'}
          onChange={(patch) =>
            setEditor((prev) =>
              prev.mode === 'closed'
                ? prev
                : { ...prev, draft: { ...prev.draft, ...patch } },
            )
          }
          onSubmit={handleSubmit}
          onCancel={() => {
            setLocalError(null)
            setEditor({ mode: 'closed' })
          }}
        />
      )}

      {commands.length === 0 && editor.mode === 'closed' && (
        <p className="nexus-saved-commands-empty">
          No saved commands yet. Click "+ New" to add one.
        </p>
      )}

      <ul className="nexus-saved-commands-list">
        {commands.map((cmd, idx) => {
          const sessionId = slugSessions[cmd.slug]
          const isRunning = Boolean(sessionId)
          const isActive = isRunning && sessionId === activeSessionId
          const isStarting = starting.has(cmd.slug)

          return (
            <li
              key={cmd.slug}
              className={[
                'nexus-saved-command',
                isRunning ? 'nexus-saved-command--running' : '',
                isActive ? 'nexus-saved-command--active' : '',
              ]
                .filter(Boolean)
                .join(' ')}
            >
              <button
                type="button"
                className="nexus-saved-command-body"
                onClick={() => void runCommand(cmd)}
                disabled={isStarting}
                title={isRunning ? 'Switch to this session' : 'Run in new session'}
              >
                <div className="nexus-saved-command-name">
                  <span
                    className={`nexus-session-dot nexus-session-dot--${isRunning ? 'running' : 'stopped'}`}
                    aria-label={isRunning ? 'Running' : 'Stopped'}
                  />
                  {cmd.name}
                  {isStarting && (
                    <span className="nexus-session-starting"> starting…</span>
                  )}
                </div>
                <code className="nexus-saved-command-cmd">{cmd.shell_cmd}</code>
                {(cmd.shell || cmd.working_dir) && (
                  <div className="nexus-saved-command-meta">
                    {cmd.shell && <span>shell: {cmd.shell}</span>}
                    {cmd.working_dir && <span>cwd: {cmd.working_dir}</span>}
                  </div>
                )}
              </button>

              <div className="nexus-saved-command-actions">
                {isRunning ? (
                  <button
                    type="button"
                    className="nexus-saved-command-stop"
                    onClick={() => void stopCommand(cmd.slug)}
                    aria-label={`Stop ${cmd.name}`}
                  >
                    Stop
                  </button>
                ) : (
                  <button
                    type="button"
                    className="nexus-saved-command-run"
                    onClick={() => void runCommand(cmd)}
                    disabled={isStarting}
                    aria-label={`Run ${cmd.name}`}
                  >
                    Run
                  </button>
                )}
                <button
                  type="button"
                  onClick={() =>
                    setEditor({
                      mode: 'edit',
                      original: cmd,
                      draft: {
                        slug: cmd.slug,
                        name: cmd.name,
                        shell: cmd.shell,
                        shell_cmd: cmd.shell_cmd,
                        working_dir: cmd.working_dir,
                        icon: cmd.icon,
                      },
                    })
                  }
                  aria-label={`Edit ${cmd.name}`}
                >
                  Edit
                </button>
                <button
                  type="button"
                  onClick={() => void handleDelete(cmd.slug)}
                  aria-label={`Delete ${cmd.name}`}
                >
                  Delete
                </button>
                <button
                  type="button"
                  disabled={idx === 0}
                  onClick={() => void handleMove(idx, -1)}
                  aria-label="Move up"
                >
                  {'↑'}
                </button>
                <button
                  type="button"
                  disabled={idx === commands.length - 1}
                  onClick={() => void handleMove(idx, 1)}
                  aria-label="Move down"
                >
                  {'↓'}
                </button>
              </div>
            </li>
          )
        })}
      </ul>
    </div>
  )
}

interface CommandFormProps {
  draft: SavedCommandDraft
  submitLabel: string
  onChange: (patch: Partial<SavedCommandDraft>) => void
  onSubmit: () => void | Promise<void>
  onCancel: () => void
}

function CommandForm(props: CommandFormProps) {
  const { draft, submitLabel, onChange, onSubmit, onCancel } = props
  return (
    <form
      className="nexus-saved-command-form"
      onSubmit={(e) => {
        e.preventDefault()
        void onSubmit()
      }}
    >
      <label>
        Name
        <input
          type="text"
          value={draft.name}
          onChange={(e) => onChange({ name: e.target.value })}
          autoFocus
        />
      </label>
      <label>
        Command
        <input
          type="text"
          value={draft.shell_cmd}
          onChange={(e) => onChange({ shell_cmd: e.target.value })}
          placeholder="npm run dev"
        />
      </label>
      <label>
        Shell (optional)
        <input
          type="text"
          value={draft.shell}
          onChange={(e) => onChange({ shell: e.target.value })}
          placeholder="/bin/bash"
        />
      </label>
      <label>
        Working dir (optional)
        <input
          type="text"
          value={draft.working_dir ?? ''}
          onChange={(e) =>
            onChange({ working_dir: e.target.value.trim() || null })
          }
          placeholder="/path/to/repo"
        />
      </label>
      <div className="nexus-saved-command-form-actions">
        <button type="submit">{submitLabel}</button>
        <button type="button" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </form>
  )
}
