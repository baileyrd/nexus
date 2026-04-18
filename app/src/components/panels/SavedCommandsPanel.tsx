// Saved commands panel (PRD-09 §14.1).
//
// Lists user-curated shell commands with run/edit/delete actions.
// "Run" finds the first live terminal session and sends the command
// as input; if no session is active it opens a terminal tab first
// and drops the input into that session once it's ready.
//
// Persistence is local to the browser today (see
// `stores/savedCommands.ts`). A follow-up PRD-09 slice migrates to
// the `com.nexus.terminal` plugin's procmgr_commands table so the
// list survives reinstall and syncs across clients.

import { useCallback, useEffect, useMemo, useState } from "react";

import {
  termCreateSession,
  termListSessions,
  termSendInput,
} from "../../ipc/terminal";
import {
  useSavedCommandsStore,
  type SavedCommand,
} from "../../stores/savedCommands";
import { useLayoutStore } from "../../stores/layout";

const EMPTY_FORM: Omit<SavedCommand, "slug"> = {
  name: "",
  shell: "",
  shellCmd: "",
  workingDir: null,
  icon: "terminal",
};

type EditorState =
  | { mode: "closed" }
  | { mode: "add"; draft: typeof EMPTY_FORM }
  | { mode: "edit"; slug: string; draft: typeof EMPTY_FORM };

async function runCommand(cmd: SavedCommand): Promise<string> {
  const existing = await termListSessions();
  let sessionId = existing[0]?.id;
  if (!sessionId) {
    sessionId = await termCreateSession({
      name: `saved:${cmd.slug}`,
      shell: cmd.shell || undefined,
      workingDir: cmd.workingDir ?? undefined,
    });
    useLayoutStore
      .getState()
      .openContentTab("terminal", "Terminal", "terminal");
  } else {
    // An active session already exists — bring the terminal tab forward
    // so the user can see the output of what's about to run.
    useLayoutStore
      .getState()
      .openContentTab("terminal", "Terminal", "terminal");
  }
  await termSendInput(sessionId, cmd.shellCmd);
  return sessionId;
}

export function SavedCommandsPanel(): JSX.Element {
  const commands = useSavedCommandsStore((s) => s.commands);
  const loaded = useSavedCommandsStore((s) => s.loaded);
  const loadError = useSavedCommandsStore((s) => s.loadError);
  const load = useSavedCommandsStore((s) => s.load);
  const add = useSavedCommandsStore((s) => s.add);
  const update = useSavedCommandsStore((s) => s.update);
  const remove = useSavedCommandsStore((s) => s.remove);
  const reorder = useSavedCommandsStore((s) => s.reorder);

  const [editor, setEditor] = useState<EditorState>({ mode: "closed" });
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!loaded) void load();
  }, [loaded, load]);

  useEffect(() => {
    if (loadError) setError(loadError);
  }, [loadError]);

  useEffect(() => {
    if (!status) return;
    const t = window.setTimeout(() => setStatus(null), 2500);
    return () => window.clearTimeout(t);
  }, [status]);

  const run = useCallback(async (cmd: SavedCommand) => {
    setError(null);
    try {
      await runCommand(cmd);
      setStatus(`Sent "${cmd.name}" to terminal`);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const copy = useCallback(async (cmd: SavedCommand) => {
    try {
      await navigator.clipboard.writeText(cmd.shellCmd);
      setStatus(`Copied "${cmd.name}"`);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const sorted = useMemo(() => commands, [commands]);

  const handleSubmit = useCallback(async () => {
    if (editor.mode === "closed") return;
    const { draft } = editor;
    if (!draft.name.trim() || !draft.shellCmd.trim()) {
      setError("Name and command are required.");
      return;
    }
    try {
      if (editor.mode === "add") {
        await add(draft);
      } else {
        await update(editor.slug, draft);
      }
      setEditor({ mode: "closed" });
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  }, [editor, add, update]);

  return (
    <div className="saved-commands-panel">
      <header className="saved-commands-header">
        <h3>Saved commands</h3>
        <button
          type="button"
          className="saved-commands-add"
          onClick={() =>
            setEditor({ mode: "add", draft: { ...EMPTY_FORM } })
          }
        >
          + New
        </button>
      </header>

      {status && <div className="saved-commands-status">{status}</div>}
      {error && <div className="saved-commands-error">{error}</div>}

      {editor.mode !== "closed" && (
        <CommandForm
          draft={editor.draft}
          onChange={(patch) =>
            setEditor((prev) =>
              prev.mode === "closed"
                ? prev
                : { ...prev, draft: { ...prev.draft, ...patch } },
            )
          }
          onSubmit={handleSubmit}
          onCancel={() => setEditor({ mode: "closed" })}
          submitLabel={editor.mode === "add" ? "Add" : "Save"}
        />
      )}

      {sorted.length === 0 && editor.mode === "closed" && (
        <p className="saved-commands-empty">
          No saved commands yet. Click "+ New" to add one.
        </p>
      )}

      <ul className="saved-commands-list">
        {sorted.map((cmd, idx) => (
          <li key={cmd.slug} className="saved-command">
            <div className="saved-command-row">
              <div className="saved-command-info">
                <div className="saved-command-name">{cmd.name}</div>
                <code className="saved-command-cmd">{cmd.shellCmd}</code>
                {(cmd.shell || cmd.workingDir) && (
                  <div className="saved-command-meta">
                    {cmd.shell && <span>shell: {cmd.shell}</span>}
                    {cmd.workingDir && <span>cwd: {cmd.workingDir}</span>}
                  </div>
                )}
              </div>
              <div className="saved-command-actions">
                <button type="button" onClick={() => void run(cmd)}>
                  Run
                </button>
                <button type="button" onClick={() => void copy(cmd)}>
                  Copy
                </button>
                <button
                  type="button"
                  onClick={() =>
                    setEditor({
                      mode: "edit",
                      slug: cmd.slug,
                      draft: {
                        name: cmd.name,
                        shell: cmd.shell,
                        shellCmd: cmd.shellCmd,
                        workingDir: cmd.workingDir,
                        icon: cmd.icon,
                      },
                    })
                  }
                >
                  Edit
                </button>
                <button
                  type="button"
                  onClick={() => {
                    remove(cmd.slug).catch((err) => setError(String(err)));
                  }}
                >
                  Delete
                </button>
                <button
                  type="button"
                  disabled={idx === 0}
                  onClick={() => {
                    reorder(cmd.slug, "up").catch((err) =>
                      setError(String(err)),
                    );
                  }}
                  aria-label="Move up"
                >
                  ↑
                </button>
                <button
                  type="button"
                  disabled={idx === sorted.length - 1}
                  onClick={() => {
                    reorder(cmd.slug, "down").catch((err) =>
                      setError(String(err)),
                    );
                  }}
                  aria-label="Move down"
                >
                  ↓
                </button>
              </div>
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}

function CommandForm(props: {
  draft: Omit<SavedCommand, "slug">;
  onChange: (patch: Partial<Omit<SavedCommand, "slug">>) => void;
  onSubmit: () => void | Promise<void>;
  onCancel: () => void;
  submitLabel: string;
}) {
  const { draft, onChange, onSubmit, onCancel, submitLabel } = props;
  return (
    <form
      className="saved-command-form"
      onSubmit={(e) => {
        e.preventDefault();
        onSubmit();
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
          value={draft.shellCmd}
          onChange={(e) => onChange({ shellCmd: e.target.value })}
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
          value={draft.workingDir ?? ""}
          onChange={(e) =>
            onChange({ workingDir: e.target.value.trim() || null })
          }
          placeholder="/path/to/repo"
        />
      </label>
      <div className="saved-command-form-actions">
        <button type="submit">{submitLabel}</button>
        <button type="button" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </form>
  );
}
