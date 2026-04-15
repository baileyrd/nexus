import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { listPlugins, type PluginSummary } from "../../../ipc/plugins";

type LoadState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; plugins: PluginSummary[] }
  | { kind: "error"; message: string };

/**
 * Read-only community/core plugins list. Pulls live data from the
 * Tauri `list_plugins` command on mount. Per-plugin settings pages and
 * install/uninstall controls will come later.
 */
export function PluginsTab() {
  const [state, setState] = useState<LoadState>({ kind: "idle" });

  useEffect(() => {
    let cancelled = false;

    function refresh() {
      listPlugins().then(
        (plugins) => {
          if (!cancelled) setState({ kind: "ready", plugins });
        },
        (err) => {
          if (!cancelled) setState({ kind: "error", message: String(err) });
        },
      );
    }

    setState({ kind: "loading" });
    refresh();

    const unlisten = listen("plugins:reloaded", () => refresh());
    return () => {
      cancelled = true;
      void unlisten.then((fn) => fn());
    };
  }, []);

  if (state.kind === "loading" || state.kind === "idle") {
    return (
      <div className="settings-tab">
        <p className="settings-empty">Loading plugins…</p>
      </div>
    );
  }
  if (state.kind === "error") {
    return (
      <div className="settings-tab">
        <p className="settings-error">Failed to load plugins: {state.message}</p>
      </div>
    );
  }

  const core = state.plugins.filter((p) => p.trust_level === "core");
  const community = state.plugins.filter((p) => p.trust_level === "community");

  return (
    <div className="settings-tab">
      <header className="settings-section-header">
        <h2>Plugins</h2>
        <p className="settings-section-desc">
          Installed plugins and their current runtime status.
        </p>
      </header>

      {core.length > 0 && (
        <PluginGroup title="Core plugins" plugins={core} />
      )}
      {community.length > 0 ? (
        <PluginGroup title="Community plugins" plugins={community} />
      ) : (
        core.length === 0 && (
          <p className="settings-empty">
            No plugins installed. Drop a plugin directory into{" "}
            <code>plugins/</code> to get started.
          </p>
        )
      )}
    </div>
  );
}

function PluginGroup({ title, plugins }: { title: string; plugins: PluginSummary[] }) {
  return (
    <section className="settings-group">
      <h3 className="settings-group-title">{title}</h3>
      <ul className="settings-rows">
        {plugins.map((p) => (
          <li key={p.id} className="settings-row">
            <div className="settings-row-body">
              <span className="settings-row-title">{p.name}</span>
              <span className="settings-row-subtitle">
                {p.id} · v{p.version}
              </span>
            </div>
            <span
              className="settings-row-badge"
              data-status={p.status}
            >
              {p.status}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}
