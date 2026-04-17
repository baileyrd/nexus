// Opens a `.bases` directory from the live forge and renders the
// first configured view in the base-view renderer stack. Reads
// `schema.json` + `records.json` + `views.toml` through
// `com.nexus.storage::base_load` (via the `load_forge_base` Tauri
// command in `forge.rs`).

import { useEffect, useState } from "react";
import { BaseViewPanel } from "./BaseView";
import { loadBase, type BaseView, type LoadedBase } from "../../ipc/database";

interface BaseFileViewProps {
  /** Forge-relative path to the `.bases` directory. */
  relpath: string;
}

type State =
  | { kind: "loading" }
  | { kind: "ready"; base: LoadedBase }
  | { kind: "error"; message: string };

export function BaseFileView({ relpath }: BaseFileViewProps) {
  const [state, setState] = useState<State>({ kind: "loading" });
  const [activeView, setActiveView] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setState({ kind: "loading" });
    loadBase(relpath).then(
      (base) => {
        if (cancelled) return;
        setState({ kind: "ready", base });
        setActiveView(base.views[0]?.name ?? null);
      },
      (err) => {
        if (!cancelled) setState({ kind: "error", message: String(err) });
      },
    );
    return () => {
      cancelled = true;
    };
  }, [relpath]);

  if (state.kind === "loading") {
    return <div className="base-view loading"><p>Loading {relpath}…</p></div>;
  }
  if (state.kind === "error") {
    return (
      <div className="base-view error" role="alert">
        <p className="label">Failed to load base</p>
        <p className="message">{state.message}</p>
      </div>
    );
  }

  const { base } = state;
  const current: BaseView | null =
    base.views.find((v) => v.name === activeView) ?? base.views[0] ?? null;

  return (
    <div className="base-view-file">
      <header className="base-view-file-header">
        <div className="base-view-file-title">
          <strong>{base.name}</strong>
          <span className="base-view-file-path">{relpath}</span>
        </div>
        {base.views.length > 1 && (
          <nav className="base-view-tabs">
            {base.views.map((v) => (
              <button
                key={v.name}
                type="button"
                className={v.name === activeView ? "tab active" : "tab"}
                onClick={() => setActiveView(v.name)}
              >
                {v.name}
              </button>
            ))}
          </nav>
        )}
      </header>
      <div className="base-view-file-body">
        {current ? (
          <BaseViewPanel
            records={base.records}
            schema={base.schema}
            view={current}
          />
        ) : (
          <div className="base-view empty">
            <p className="label">{base.name}</p>
            <p className="hint">
              This base has no configured views. Add one to `views.toml`
              to get started.
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
