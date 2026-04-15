import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invokePluginCommand } from "../../ipc/plugins";

interface PluginPanelProps {
  pluginId: string;
  panelId: string;
}

type LoadState =
  | { kind: "loading" }
  | { kind: "ready"; content: string }
  | { kind: "error"; message: string };

/**
 * Renders a plugin-contributed side panel. Calls the plugin's
 * `ui_panel` handler on mount and again on every `plugins:reloaded`
 * event, then renders the `content` string the handler returns.
 *
 * Expected handler payload: `{ content: "..." }` (string). Anything
 * else is shown as a fallback error so a malformed plugin can't crash
 * the side panel.
 */
export function PluginPanel({ pluginId, panelId }: PluginPanelProps) {
  const [state, setState] = useState<LoadState>({ kind: "loading" });

  useEffect(() => {
    let cancelled = false;

    function refresh() {
      setState({ kind: "loading" });
      invokePluginCommand(pluginId, panelId).then(
        (result) => {
          if (cancelled) return;
          const content = extractContent(result);
          setState(content === null
            ? { kind: "error", message: "plugin returned no content" }
            : { kind: "ready", content });
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
  }, [pluginId, panelId]);

  if (state.kind === "loading") {
    return <div className="plugin-panel loading">Loading…</div>;
  }
  if (state.kind === "error") {
    return (
      <div className="plugin-panel error" role="alert">
        {state.message}
      </div>
    );
  }
  return <pre className="plugin-panel-content">{state.content}</pre>;
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
