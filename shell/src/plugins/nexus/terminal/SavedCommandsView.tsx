// shell/src/plugins/nexus/terminal/SavedCommandsView.tsx
//
// WI-05 — sub-view of nexus.terminal that lists user-saved shell
// commands with CRUD + reorder + click-to-execute. Mirrors the legacy
// SavedCommandsPanel UX (app/src/components/panels/SavedCommandsPanel.tsx)
// without copying the implementation.
//
// Lives as a sidebar leaf (side: 'left') alongside the terminal so the
// user can keep the terminal visible while picking a command. Clicking
// a command sends it to the active terminal session via
// `com.nexus.terminal::send_input`; if no session exists we ask the
// terminal plugin to create one (via the existing focus command) and
// retry once.
//
// Reorder is up/down buttons rather than HTML5 drag-drop. The legacy
// panel used the same affordance and it survives keyboard navigation;
// drag-drop reorder is a Phase 3 polish item.

import { useCallback, useEffect, useMemo, useState } from 'react'
import type { KernelAPI, NotificationsAPI } from '../../../types/plugin'
import { useTerminalStore } from './terminalStore'
import {
  useSavedCommandsStore,
  type SavedCommand,
  type SavedCommandDraft,
} from './savedCommandsStore'

const PLUGIN_ID = 'com.nexus.terminal'
// `send_input` (HANDLER_SEND_INPUT = 3) appends a newline if the input
// doesn't already end in one — exactly what we want for click-to-run.
const CMD_SEND_INPUT = 'send_input'

interface SavedCommandsViewProps {
  kernel: KernelAPI
  notifications: NotificationsAPI
  /** Called when the user wants to open the terminal pane (no active
   *  session yet, or just-ran-a-command UX). The plugin's index.ts
   *  registers this command (`nexus.terminal.focus`). */
  focusTerminal: () => void
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

/** Slugify a freeform name into a URL-safe primary key. The kernel does
 *  not enforce any specific shape — it just uses `slug` as the rowid —
 *  but a-z0-9-dash keeps URLs and file paths sane if the slug ever
 *  shows up in either. Falls back to a timestamp suffix on conflict. */
function slugify(name: string): string {
  const base = name
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
  return base || `cmd-${Date.now().toString(36)}`
}

export function SavedCommandsView(props: SavedCommandsViewProps) {
  const { kernel, notifications, focusTerminal } = props
  const commands = useSavedCommandsStore((s) => s.commands)
  const loaded = useSavedCommandsStore((s) => s.loaded)
  const error = useSavedCommandsStore((s) => s.error)
  const loadSaved = useSavedCommandsStore((s) => s.loadSaved)
  const createSaved = useSavedCommandsStore((s) => s.createSaved)
  const updateSaved = useSavedCommandsStore((s) => s.updateSaved)
  const deleteSaved = useSavedCommandsStore((s) => s.deleteSaved)
  const reorderSaved = useSavedCommandsStore((s) => s.reorderSaved)

  const [editor, setEditor] = useState<EditorState>({ mode: 'closed' })
  const [localError, setLocalError] = useState<string | null>(null)

  // Hydrate on first mount (and any subsequent re-mount after the leaf
  // was torn down). Cheap if already loaded — `saved_list` returns from
  // an in-process sqlite store with no IO churn.
  useEffect(() => {
    if (!loaded) void loadSaved(kernel)
  }, [loaded, loadSaved, kernel])

  const slugSet = useMemo(() => new Set(commands.map((c) => c.slug)), [commands])

  /** Pick a slug that doesn't collide with an existing row. Append a
   *  short suffix on collision — guarantees forward progress without
   *  asking the user to retype. */
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

  const runCommand = useCallback(
    async (cmd: SavedCommand) => {
      setLocalError(null)
      const sessionId = useTerminalStore.getState().sessionId
      if (!sessionId) {
        // No live session — open the terminal so the workspace handler
        // creates one, then surface a hint. We deliberately don't loop
        // here: the create path is async and racing it would risk
        // double-sending. The user clicks again once the terminal is
        // up.
        focusTerminal()
        notifications.show({
          message: 'Opening terminal — click the command again to run it.',
          type: 'info',
          duration: 3000,
        })
        return
      }
      try {
        await kernel.invoke(PLUGIN_ID, CMD_SEND_INPUT, {
          id: sessionId,
          input: cmd.shell_cmd,
        })
        focusTerminal()
        notifications.show({
          message: `Sent "${cmd.name}" to terminal`,
          type: 'success',
          duration: 1800,
        })
      } catch (err) {
        setLocalError(String(err))
      }
    },
    [kernel, notifications, focusTerminal],
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
        // Pick the slug at submit time, not on every keystroke, so the
        // user can rename freely without our slug churning under them.
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
      setLocalError(null)
      try {
        await deleteSaved(kernel, slug)
      } catch (err) {
        setLocalError(String(err))
      }
    },
    [deleteSaved, kernel],
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
        await reorderSaved(
          kernel,
          next.map((c) => c.slug),
        )
      } catch (err) {
        setLocalError(String(err))
      }
    },
    [commands, reorderSaved, kernel],
  )

  return (
    <div className="nexus-saved-commands">
      <header className="nexus-saved-commands-header">
        <h3>Saved commands</h3>
        <button
          type="button"
          className="nexus-saved-commands-add"
          onClick={() => setEditor({ mode: 'add', draft: { ...EMPTY_DRAFT } })}
        >
          + New
        </button>
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
        {commands.map((cmd, idx) => (
          <li key={cmd.slug} className="nexus-saved-command">
            <button
              type="button"
              className="nexus-saved-command-body"
              onClick={() => void runCommand(cmd)}
              title="Run in active terminal"
            >
              <div className="nexus-saved-command-name">{cmd.name}</div>
              <code className="nexus-saved-command-cmd">{cmd.shell_cmd}</code>
              {(cmd.shell || cmd.working_dir) && (
                <div className="nexus-saved-command-meta">
                  {cmd.shell && <span>shell: {cmd.shell}</span>}
                  {cmd.working_dir && <span>cwd: {cmd.working_dir}</span>}
                </div>
              )}
            </button>
            <div className="nexus-saved-command-actions">
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
        ))}
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
