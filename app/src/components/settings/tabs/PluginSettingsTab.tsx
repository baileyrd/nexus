import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invokePluginCommand, type PluginUiSettingsTab } from "../../../ipc/plugins";

interface PluginSettingsTabProps {
  /** The contribution this tab renders. */
  tab: PluginUiSettingsTab;
}

type LoadState =
  | { kind: "loading" }
  | { kind: "ready"; content: string }
  | { kind: "error"; message: string };

/**
 * Per-plugin Settings tab. Renders a short auto-generated header
 * (plugin name + id + version) followed by whatever string the
 * plugin's handler returns as `content`. Re-fetches on
 * `plugins:reloaded` so WASM changes reflect live.
 */
export function PluginSettingsTab({ tab }: PluginSettingsTabProps) {
  const [state, setState] = useState<LoadState>({ kind: "loading" });

  useEffect(() => {
    let cancelled = false;

    function refresh() {
      setState({ kind: "loading" });
      invokePluginCommand(tab.plugin_id, tab.tab_id).then(
        (result) => {
          if (cancelled) return;
          const content = extractContent(result);
          setState(
            content === null
              ? { kind: "error", message: "plugin returned no content" }
              : { kind: "ready", content },
          );
        },
        (err) => {
          if (!cancelled) setState({ kind: "error", message: String(err) });
        },
      );
    }

    refresh();
    const unlisten = listen("plugins:reloaded", () => refresh());
    return () => {
      cancelled = true;
      void unlisten.then((fn) => fn());
    };
  }, [tab.plugin_id, tab.tab_id]);

  return (
    <div className="settings-tab">
      <header className="settings-section-header">
        <h2>{tab.title}</h2>
        <p className="settings-section-desc">
          {tab.plugin_name} <code>{tab.plugin_id}</code> · v{tab.plugin_version}
        </p>
      </header>

      {state.kind === "loading" && <p className="settings-empty">Loading…</p>}
      {state.kind === "error" && (
        <p className="settings-error">{state.message}</p>
      )}
      {state.kind === "ready" && (
        <pre className="plugin-settings-content">{state.content}</pre>
      )}
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
