// shell/src/plugins/nexus/terminal/SavedCommandsView.tsx
//
// WI-05 — sub-view of nexus.terminal that lists user-saved shell
// commands with CRUD + reorder + click-to-execute. Mirrors the legacy
// SavedCommandsPanel UX (from the legacy shell's SavedCommandsPanel.tsx, retired Phase 4 WI-37)
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
import { useConfigValue } from '../../../stores/configStore'
import { Icon } from '../../../icons'
import { useTerminalStore } from './terminalStore'

const CMD_SAVE_NOTIFICATION_MS = 3000
const CMD_COPIED_NOTIFICATION_MS = 1800
import {
  fetchRunningSavedSessions,
  restartSavedSession,
  spawnSavedSession,
  stopSavedSession,
  useSavedCommandsStore,
  type SavedCommand,
  type SavedCommandDraft,
} from './savedCommandsStore'

const PLUGIN_ID = 'com.nexus.terminal'
// `send_input` (HANDLER_SEND_INPUT = 3) appends a newline if the input
// doesn't already end in one — exactly what we want for click-to-run.
const CMD_SEND_INPUT = 'send_input'
// BL-059 — `open_in_terminal` (HANDLER_OPEN_IN_TERMINAL = 18) hands the
// saved command's `working_dir` off to the user's preferred external
// terminal emulator.
const CMD_OPEN_IN_TERMINAL = 'open_in_terminal'

// BL-066 follow-up — interval (ms) the running-session poller uses to
// refresh the slug → session-id map from `list_sessions`. Two seconds
// matches the legacy panel's polling cadence and keeps the spawn → "●
// running" lag below human perception without burning the kernel.
const RUNNING_POLL_INTERVAL_MS = 2_000

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
  env_vars: {},
}

/**
 * BL-059 follow-up — encode an `env_vars` map as one `KEY=VALUE`
 * line per pair for the editor textarea. Keys sort alphabetically
 * so a save / reopen cycle stays stable.
 */
export function envVarsToText(env: Record<string, string>): string {
  return Object.keys(env)
    .sort()
    .map((k) => `${k}=${env[k]}`)
    .join('\n')
}

/**
 * BL-059 follow-up — parse the editor textarea back into an
 * `env_vars` map. Empty / whitespace-only / comment (`#`) lines are
 * skipped; the first `=` splits key from value; the value is taken
 * verbatim (no quote-stripping — Bash interprets quoted env values
 * as literal text and we match that). Lines without `=` are dropped
 * with the rest of the noise.
 */
export function parseEnvVars(text: string): Record<string, string> {
  const out: Record<string, string> = {}
  for (const raw of text.split('\n')) {
    const line = raw.trim()
    if (!line || line.startsWith('#')) continue
    const eq = line.indexOf('=')
    if (eq <= 0) continue
    const key = line.slice(0, eq).trim()
    if (!key) continue
    out[key] = line.slice(eq + 1)
  }
  return out
}

type EditorState =
  | { mode: 'closed' }
  | { mode: 'add'; draft: SavedCommandDraft }
  | { mode: 'edit'; original: SavedCommand; draft: SavedCommandDraft }

/**
 * BL-059 follow-up — split the `terminal.externalPriority` setting
 * into a sanitized list. Accepts comma- or whitespace-separated
 * tokens, normalises kebab to snake (the kernel's `parse_kind`
 * accepts both), filters out empties + duplicates + obviously
 * unsupported tags. Unrecognised tokens are silently dropped — the
 * kernel would error otherwise and the user's intent ("don't use
 * what I haven't whitelisted") is still honoured.
 */
const KNOWN_TERMINAL_KINDS: ReadonlySet<string> = new Set([
  'iterm2',
  'iterm',
  'wezterm',
  'ghostty',
  'kitty',
  'alacritty',
  'windows_terminal',
  'wt',
  'gnome_terminal',
  'konsole',
  'xfce4_terminal',
  'mac_terminal',
  'terminal',
  'x_terminal_emulator',
  'xterm',
])

export function parseExternalPriority(raw: string): string[] {
  if (!raw) return []
  const tokens = raw
    .split(/[,\s]+/)
    .map((t) => t.trim().toLowerCase().replace(/-/g, '_'))
    .filter((t) => t.length > 0)
  const out: string[] = []
  const seen = new Set<string>()
  for (const t of tokens) {
    if (!KNOWN_TERMINAL_KINDS.has(t)) continue
    if (seen.has(t)) continue
    seen.add(t)
    out.push(t)
  }
  return out
}

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
  const cmdSaveMs = useConfigValue('ui.commandSaveNotificationMs', CMD_SAVE_NOTIFICATION_MS)
  const cmdCopiedMs = useConfigValue('ui.commandCopiedNotificationMs', CMD_COPIED_NOTIFICATION_MS)
  // BL-059 follow-up — comma-separated emulator priority. Empty
  // strings collapse to "use the kernel default". `parseExternalPriority`
  // is exported as a pure helper so a unit test can pin the
  // splitting + canonicalisation rules without driving React.
  const externalPriorityRaw = useConfigValue('terminal.externalPriority', '')
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
  // BL-066 follow-up — slug → live `saved:<slug>` session ids. Empty
  // (or missing) means the saved command has no managed session
  // currently running, in which case the row's Stop / Restart icons
  // stay hidden and only Spawn is visible.
  const [running, setRunning] = useState<Record<string, string[]>>({})

  // Hydrate on first mount (and any subsequent re-mount after the leaf
  // was torn down). Cheap if already loaded — `saved_list` returns from
  // an in-process sqlite store with no IO churn.
  useEffect(() => {
    if (!loaded) void loadSaved(kernel)
  }, [loaded, loadSaved, kernel])

  // BL-066 follow-up — poll `list_sessions` so the row icons can
  // surface live state ("● running" + Stop / Restart). Unconditional
  // poll on mount + every `RUNNING_POLL_INTERVAL_MS` ms, cleared on
  // unmount so the leaf doesn't burn IPC after the user collapses
  // the sidebar.
  useEffect(() => {
    let cancelled = false
    const refresh = async () => {
      try {
        const map = await fetchRunningSavedSessions(kernel)
        if (!cancelled) setRunning(map)
      } catch {
        // Swallow — a missing / restarting terminal plugin is the
        // common no-op case and shouldn't flicker the UI's error
        // pane every 2 s.
      }
    }
    void refresh()
    const handle = window.setInterval(() => void refresh(), RUNNING_POLL_INTERVAL_MS)
    return () => {
      cancelled = true
      clearInterval(handle)
    }
  }, [kernel])

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
          duration: cmdSaveMs ?? CMD_SAVE_NOTIFICATION_MS,
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
          duration: cmdCopiedMs ?? CMD_COPIED_NOTIFICATION_MS,
        })
      } catch (err) {
        setLocalError(String(err))
      }
    },
    [kernel, notifications, focusTerminal, cmdSaveMs, cmdCopiedMs],
  )

  // BL-066 follow-up — refresh the live `saved:<slug>` map immediately
  // after a lifecycle action so the icons flip without waiting for the
  // next 2 s poll tick. Best-effort; failure leaves the next poll to
  // converge.
  const refreshRunning = useCallback(async () => {
    try {
      const map = await fetchRunningSavedSessions(kernel)
      setRunning(map)
    } catch {
      // ignore — the periodic poller will pick up the change.
    }
  }, [kernel])

  // BL-066 follow-up — spawn a fresh managed PTY session via BL-055's
  // `run_saved`. Distinct from the existing Run button (which sends
  // the command line into the *active* terminal via `send_input`):
  // this affordance is for long-running services the user wants to
  // Stop / Restart later.
  const spawnManaged = useCallback(
    async (cmd: SavedCommand) => {
      setLocalError(null)
      try {
        await spawnSavedSession(kernel, cmd.slug)
        notifications.show({
          message: `Spawned managed session for "${cmd.name}"`,
          type: 'success',
          duration: cmdCopiedMs ?? CMD_COPIED_NOTIFICATION_MS,
        })
        await refreshRunning()
      } catch (err) {
        setLocalError(String(err))
      }
    },
    [kernel, notifications, cmdCopiedMs, refreshRunning],
  )

  // BL-066 follow-up — close every PTY session whose name matches
  // `saved:<slug>`. Concurrent spawns are rare but not impossible
  // (workflow + user-driven), so the helper iterates ids rather than
  // assuming a single match.
  const stopManaged = useCallback(
    async (cmd: SavedCommand) => {
      setLocalError(null)
      const ids = running[cmd.slug] ?? []
      if (ids.length === 0) return
      try {
        await stopSavedSession(kernel, ids)
        notifications.show({
          message: `Stopped managed session for "${cmd.name}"`,
          type: 'info',
          duration: cmdCopiedMs ?? CMD_COPIED_NOTIFICATION_MS,
        })
        await refreshRunning()
      } catch (err) {
        setLocalError(String(err))
      }
    },
    [kernel, notifications, running, cmdCopiedMs, refreshRunning],
  )

  // BL-066 follow-up — Stop + spawn. The store helper sequences the
  // close before the new spawn so the user-visible "● running" pip
  // cleanly transitions through 0 between attempts.
  const restartManaged = useCallback(
    async (cmd: SavedCommand) => {
      setLocalError(null)
      const ids = running[cmd.slug] ?? []
      try {
        await restartSavedSession(kernel, cmd.slug, ids)
        notifications.show({
          message: `Restarted managed session for "${cmd.name}"`,
          type: 'success',
          duration: cmdCopiedMs ?? CMD_COPIED_NOTIFICATION_MS,
        })
        await refreshRunning()
      } catch (err) {
        setLocalError(String(err))
      }
    },
    [kernel, notifications, running, cmdCopiedMs, refreshRunning],
  )

  // BL-059 — open the saved command's working directory in the user's
  // preferred external terminal emulator (vim/htop/REPLs that don't
  // play nicely under the in-app PTY). Backend handles detection +
  // detached spawn; we just surface success/failure.
  const openInExternalTerminal = useCallback(
    async (cmd: SavedCommand) => {
      setLocalError(null)
      if (!cmd.working_dir) {
        setLocalError(
          `"${cmd.name}" has no working directory; set one in the Edit dialog first.`,
        )
        return
      }
      const priority = parseExternalPriority(externalPriorityRaw ?? '')
      const args: { slug: string; priority?: string[] } = { slug: cmd.slug }
      if (priority.length > 0) args.priority = priority
      try {
        const resp = await kernel.invoke<{
          kind: string
          program: string
          working_dir: string
        }>(PLUGIN_ID, CMD_OPEN_IN_TERMINAL, args)
        notifications.show({
          message: `Opened ${resp.program} at ${resp.working_dir}`,
          type: 'success',
          duration: cmdCopiedMs ?? CMD_COPIED_NOTIFICATION_MS,
        })
      } catch (err) {
        setLocalError(String(err))
      }
    },
    [kernel, notifications, cmdCopiedMs, externalPriorityRaw],
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
        {commands.map((cmd, idx) => {
          const runningIds = running[cmd.slug] ?? []
          const isRunning = runningIds.length > 0
          return (
          <li key={cmd.slug} className="nexus-saved-command">
            <button
              type="button"
              className="nexus-saved-command-body"
              onClick={() => void runCommand(cmd)}
              title="Run in active terminal"
            >
              <div className="nexus-saved-command-name">
                {/* BL-066 follow-up — green dot when one or more
                    `saved:<slug>` sessions are live. Driven by the
                    `running` poll above. */}
                {isRunning && (
                  <span
                    className="nexus-saved-command-running-dot"
                    aria-label={
                      runningIds.length === 1
                        ? '1 managed session running'
                        : `${runningIds.length} managed sessions running`
                    }
                    title={
                      runningIds.length === 1
                        ? '1 managed session running'
                        : `${runningIds.length} managed sessions running`
                    }
                  >
                    {'●'}
                  </span>
                )}
                {cmd.name}
              </div>
              <code className="nexus-saved-command-cmd">{cmd.shell_cmd}</code>
              {(cmd.shell || cmd.working_dir) && (
                <div className="nexus-saved-command-meta">
                  {cmd.shell && <span>shell: {cmd.shell}</span>}
                  {cmd.working_dir && <span>cwd: {cmd.working_dir}</span>}
                </div>
              )}
            </button>
            {/* BL-066: hover-revealed icon row. Run sends to the active
                terminal (send_input); Spawn / Stop / Restart manage a
                fresh PTY session named `saved:<slug>` via BL-055's
                run_saved + the standard list_sessions / close_session
                verbs. Stop and Restart only render when at least one
                matching session is currently live. */}
            <div className="nexus-saved-command-actions">
              <button
                type="button"
                onClick={() => void runCommand(cmd)}
                aria-label={`Run ${cmd.name} in active terminal`}
                title="Run in active terminal"
              >
                <Icon name="play" size={12} />
              </button>
              <button
                type="button"
                onClick={() => void spawnManaged(cmd)}
                aria-label={`Spawn managed session for ${cmd.name}`}
                title="Spawn managed session"
              >
                <Icon name="bolt" size={12} />
              </button>
              {isRunning && (
                <button
                  type="button"
                  onClick={() => void stopManaged(cmd)}
                  aria-label={`Stop managed session for ${cmd.name}`}
                  title="Stop managed session"
                >
                  <Icon name="stop" size={10} />
                </button>
              )}
              {isRunning && (
                <button
                  type="button"
                  onClick={() => void restartManaged(cmd)}
                  aria-label={`Restart managed session for ${cmd.name}`}
                  title="Restart managed session"
                >
                  <Icon name="refresh" size={12} />
                </button>
              )}
              {cmd.working_dir && (
                <button
                  type="button"
                  onClick={() => void openInExternalTerminal(cmd)}
                  aria-label={`Open ${cmd.name} in external terminal`}
                  title="Open in external terminal"
                >
                  <Icon name="external" size={12} />
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
                      env_vars: cmd.env_vars,
                    },
                  })
                }
                aria-label={`Edit ${cmd.name}`}
                title="Edit"
              >
                <Icon name="pencil" size={12} />
              </button>
              <button
                type="button"
                disabled={idx === 0}
                onClick={() => void handleMove(idx, -1)}
                aria-label="Move up"
                title="Move up"
              >
                {'↑'}
              </button>
              <button
                type="button"
                disabled={idx === commands.length - 1}
                onClick={() => void handleMove(idx, 1)}
                aria-label="Move down"
                title="Move down"
              >
                {'↓'}
              </button>
              <button
                type="button"
                className="is-destructive"
                onClick={() => void handleDelete(cmd.slug)}
                aria-label={`Delete ${cmd.name}`}
                title="Delete"
              >
                <Icon name="trash" size={12} />
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
      <label>
        Env vars (one per line, KEY=VALUE)
        <textarea
          value={envVarsToText(draft.env_vars)}
          onChange={(e) => onChange({ env_vars: parseEnvVars(e.target.value) })}
          placeholder="DEBUG=1\nNODE_ENV=development"
          rows={3}
          spellCheck={false}
          style={{ fontFamily: 'var(--font-monospace)', fontSize: 12 }}
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
