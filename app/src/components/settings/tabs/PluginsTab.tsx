import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  listPlugins,
  togglePluginSubscription,
  type PluginSummary,
  type SubscriptionSummary,
} from "../../../ipc/plugins";

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

  function handleToggle(pluginId: string, subId: string, enabled: boolean) {
    togglePluginSubscription(pluginId, subId, enabled).then(() => {
      // Optimistically update local state.
      setState((prev) => {
        if (prev.kind !== "ready") return prev;
        return {
          kind: "ready",
          plugins: prev.plugins.map((p) =>
            p.id === pluginId
              ? {
                  ...p,
                  event_subscriptions: p.event_subscriptions.map((s) =>
                    s.id === subId ? { ...s, enabled } : s,
                  ),
                }
              : p,
          ),
        };
      });
    });
  }

  return (
    <div className="settings-tab">
      <header className="settings-section-header">
        <h2>Plugins</h2>
        <p className="settings-section-desc">
          Installed plugins and their current runtime status.
        </p>
      </header>

      {core.length > 0 && (
        <PluginGroup title="Core plugins" plugins={core} onToggle={handleToggle} />
      )}
      {community.length > 0 ? (
        <PluginGroup title="Community plugins" plugins={community} onToggle={handleToggle} />
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

function PluginGroup({
  title,
  plugins,
  onToggle,
}: {
  title: string;
  plugins: PluginSummary[];
  onToggle: (pluginId: string, subId: string, enabled: boolean) => void;
}) {
  return (
    <section className="settings-group">
      <h3 className="settings-group-title">{title}</h3>
      <ul className="settings-rows">
        {plugins.map((p) => (
          <li key={p.id} className="settings-row settings-row--expandable">
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
            {p.event_subscriptions.length > 0 && (
              <SubscriptionList
                pluginId={p.id}
                subscriptions={p.event_subscriptions}
                onToggle={onToggle}
              />
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}

function SubscriptionList({
  pluginId,
  subscriptions,
  onToggle,
}: {
  pluginId: string;
  subscriptions: SubscriptionSummary[];
  onToggle: (pluginId: string, subId: string, enabled: boolean) => void;
}) {
  return (
    <div className="settings-sub-rows">
      <span className="settings-sub-rows-label">Event subscriptions</span>
      {subscriptions.map((sub) => (
        <label key={sub.id} className="settings-sub-row">
          <span className="settings-sub-row-filter" title={sub.id}>
            {sub.filter}
          </span>
          <input
            type="checkbox"
            className="settings-toggle"
            checked={sub.enabled}
            onChange={(e) => onToggle(pluginId, sub.id, e.target.checked)}
          />
        </label>
      ))}
    </div>
  );
}
