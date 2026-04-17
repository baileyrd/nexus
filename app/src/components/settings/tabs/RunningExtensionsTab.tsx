import { useEffect, useState } from "react";
import { listPlugins, type PluginSummary } from "../../../ipc/plugins";
import { usePluginStatusStore } from "../../../plugins/status";

/**
 * Settings → Plugins → "Running Extensions" tab (UI F-10.1.1).
 *
 * Enumerates every loaded plugin with its runtime tier, trust level,
 * backend status, and the live health signal from
 * `usePluginStatusStore` — most recent error, slowest observed command,
 * lifecycle-hook timings. The tab is read-only today; a future PR wires
 * disable / uninstall buttons.
 */
export function RunningExtensionsTab() {
  const [plugins, setPlugins] = useState<PluginSummary[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const status = usePluginStatusStore((s) => s.entries);

  useEffect(() => {
    let cancelled = false;
    listPlugins()
      .then((list) => {
        if (!cancelled) setPlugins(list);
      })
      .catch((err) => {
        if (!cancelled) setLoadError(String(err));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (loadError) {
    return (
      <div className="settings-tab running-extensions-tab">
        <h3>Running extensions</h3>
        <p role="alert" className="error">
          Failed to load plugin list: {loadError}
        </p>
      </div>
    );
  }

  if (plugins === null) {
    return (
      <div className="settings-tab running-extensions-tab">
        <h3>Running extensions</h3>
        <p role="status">Loading…</p>
      </div>
    );
  }

  if (plugins.length === 0) {
    return (
      <div className="settings-tab running-extensions-tab">
        <h3>Running extensions</h3>
        <p>(no plugins loaded)</p>
      </div>
    );
  }

  return (
    <div className="settings-tab running-extensions-tab">
      <h3>Running extensions</h3>
      <p className="muted" style={{ fontSize: "0.9em" }}>
        Live diagnostics for every loaded plugin — runtime tier, trust
        level, status, most recent error, slowest observed command,
        lifecycle timings.
      </p>
      <ul
        className="plugin-status-list"
        style={{ listStyle: "none", padding: 0 }}
      >
        {plugins.map((p) => {
          const s = status[p.id];
          const badge: string =
            s?.level === "failed"
              ? "●"
              : s?.level === "slow"
                ? "○"
                : "·";
          return (
            <li
              key={p.id}
              style={{
                padding: "8px 0",
                borderBottom: "1px solid var(--border-subtle, #ddd)",
              }}
            >
              <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
                <span
                  aria-hidden
                  title={s?.level ?? "ok"}
                  style={{
                    color:
                      s?.level === "failed"
                        ? "var(--color-error, #c33)"
                        : s?.level === "slow"
                          ? "var(--color-warn, #c80)"
                          : "var(--color-ok, #393)",
                  }}
                >
                  {badge}
                </span>
                <strong>{p.name}</strong>
                <span className="muted" style={{ fontSize: "0.85em" }}>
                  {p.id} · {p.version}
                </span>
              </div>
              <div style={{ fontSize: "0.85em", opacity: 0.8, marginTop: 4 }}>
                {p.trust_level} · {p.runtime} · {p.status}
              </div>
              {s?.lifecycleMs && (
                <div style={{ fontSize: "0.85em", opacity: 0.8 }}>
                  cold start:{" "}
                  {s.lifecycleMs.onInit !== undefined &&
                    `onInit ${s.lifecycleMs.onInit.toFixed(0)}ms`}
                  {s.lifecycleMs.onInit !== undefined &&
                    s.lifecycleMs.onStart !== undefined &&
                    " · "}
                  {s.lifecycleMs.onStart !== undefined &&
                    `onStart ${s.lifecycleMs.onStart.toFixed(0)}ms`}
                </div>
              )}
              {s?.slowestCommandMs !== undefined && (
                <div style={{ fontSize: "0.85em", opacity: 0.8 }}>
                  slowest command: {s.slowestCommandId ?? "(unknown)"} —{" "}
                  {s.slowestCommandMs.toFixed(0)}ms
                </div>
              )}
              {s?.lastError && (
                <div
                  style={{
                    fontSize: "0.85em",
                    color: "var(--color-error, #c33)",
                    marginTop: 4,
                  }}
                >
                  last error: {s.lastError}
                </div>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
