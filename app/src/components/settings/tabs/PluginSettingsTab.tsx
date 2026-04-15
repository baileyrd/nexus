import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invokePluginCommand, type PluginUiSettingsTab } from "../../../ipc/plugins";
import {
  getPluginSettings,
  getPluginSettingsSchema,
  savePluginSettings,
  type JsonSchema,
} from "../../../ipc/pluginSettings";
import { SettingsSchemaForm } from "../SettingsSchemaForm";

interface PluginSettingsTabProps {
  /** The contribution this tab renders. */
  tab: PluginUiSettingsTab;
}

type ContentState =
  | { kind: "loading" }
  | { kind: "ready"; content: string | null }
  | { kind: "error"; message: string };

type FormState =
  | { kind: "none" }
  | { kind: "loading" }
  | {
      kind: "ready";
      schema: JsonSchema;
      values: Record<string, unknown>;
      saved: Record<string, unknown>;
      status: SaveStatus;
    }
  | { kind: "error"; message: string };

type SaveStatus = "idle" | "saving" | "saved" | { error: string };

/**
 * Per-plugin Settings tab. If the plugin declares a `[settings]`
 * schema, renders a form driven by that schema and wires Save →
 * `save_plugin_settings`. Falls back to — or additionally renders —
 * whatever string the plugin's `ui_settings_tab` handler returns.
 * Refetches both sides on `plugins:reloaded`.
 */
export function PluginSettingsTab({ tab }: PluginSettingsTabProps) {
  const [contentState, setContentState] = useState<ContentState>({ kind: "loading" });
  const [formState, setFormState] = useState<FormState>({ kind: "loading" });

  useEffect(() => {
    let cancelled = false;

    async function refresh() {
      setContentState({ kind: "loading" });
      setFormState({ kind: "loading" });

      // Fire both requests in parallel — one tab, two pieces.
      const contentPromise = invokePluginCommand(tab.plugin_id, tab.tab_id).then(
        (result): ContentState => ({ kind: "ready", content: extractContent(result) }),
        (err): ContentState => ({ kind: "error", message: String(err) }),
      );

      const formPromise: Promise<FormState> = (async () => {
        try {
          const schema = await getPluginSettingsSchema(tab.plugin_id);
          if (!schema) return { kind: "none" };
          const values = await getPluginSettings(tab.plugin_id);
          return {
            kind: "ready",
            schema,
            values,
            saved: values,
            status: "idle",
          };
        } catch (err) {
          return { kind: "error", message: String(err) };
        }
      })();

      const [nextContent, nextForm] = await Promise.all([contentPromise, formPromise]);
      if (cancelled) return;
      setContentState(nextContent);
      setFormState(nextForm);
    }

    void refresh();
    const unlisten = listen("plugins:reloaded", () => {
      void refresh();
    });
    return () => {
      cancelled = true;
      void unlisten.then((fn) => fn());
    };
  }, [tab.plugin_id, tab.tab_id]);

  async function handleSave() {
    if (formState.kind !== "ready") return;
    setFormState({ ...formState, status: "saving" });
    try {
      await savePluginSettings(tab.plugin_id, formState.values);
      setFormState({
        ...formState,
        saved: formState.values,
        status: "saved",
      });
    } catch (err) {
      setFormState({ ...formState, status: { error: String(err) } });
    }
  }

  function handleRevert() {
    if (formState.kind !== "ready") return;
    setFormState({ ...formState, values: formState.saved, status: "idle" });
  }

  return (
    <div className="settings-tab">
      <header className="settings-section-header">
        <h2>{tab.title}</h2>
        <p className="settings-section-desc">
          {tab.plugin_name} <code>{tab.plugin_id}</code> · v{tab.plugin_version}
        </p>
      </header>

      {formState.kind === "loading" && <p className="settings-empty">Loading settings…</p>}
      {formState.kind === "error" && (
        <p className="settings-error">Settings: {formState.message}</p>
      )}
      {formState.kind === "ready" && (
        <section className="plugin-settings-section">
          <SettingsSchemaForm
            schema={formState.schema}
            values={formState.values}
            onChange={(next) =>
              setFormState({ ...formState, values: next, status: "idle" })
            }
          />
          <FormActions
            dirty={!deepEqual(formState.values, formState.saved)}
            status={formState.status}
            onSave={handleSave}
            onRevert={handleRevert}
          />
        </section>
      )}

      {contentState.kind === "loading" && formState.kind !== "ready" && (
        <p className="settings-empty">Loading content…</p>
      )}
      {contentState.kind === "error" && (
        <p className="settings-error">Content: {contentState.message}</p>
      )}
      {contentState.kind === "ready" && contentState.content !== null && (
        <pre className="plugin-settings-content">{contentState.content}</pre>
      )}
    </div>
  );
}

interface FormActionsProps {
  dirty: boolean;
  status: SaveStatus;
  onSave: () => void;
  onRevert: () => void;
}

function FormActions({ dirty, status, onSave, onRevert }: FormActionsProps) {
  const saving = status === "saving";
  const saved = status === "saved";
  const error = typeof status === "object" ? status.error : null;

  return (
    <div className="plugin-settings-actions">
      <button
        type="button"
        className="plugin-settings-save"
        disabled={!dirty || saving}
        onClick={onSave}
      >
        {saving ? "Saving…" : "Save"}
      </button>
      <button
        type="button"
        className="plugin-settings-revert"
        disabled={!dirty || saving}
        onClick={onRevert}
      >
        Revert
      </button>
      {saved && !dirty && <span className="plugin-settings-saved-hint">Saved.</span>}
      {error && <span className="settings-error">{error}</span>}
    </div>
  );
}

function extractContent(result: unknown): string | null {
  if (
    typeof result === "object" &&
    result !== null &&
    "content" in result &&
    typeof (result as { content: unknown }).content === "string"
  ) {
    return (result as { content: string }).content;
  }
  return null;
}

/** Shallow deep-equal — sufficient for the flat settings objects we handle. */
function deepEqual(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}
