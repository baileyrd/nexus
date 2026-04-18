// Workflows browser panel (PRD-16).
//
// Two-pane read-only view over `com.nexus.workflow::list` / `::get`:
// left column lists every loaded workflow; right column shows its
// trigger, condition, and step pipeline. "Reload" re-scans the
// forge's `.workflows/` tree through the plugin.

import { useCallback, useEffect, useMemo, useState } from "react";

import { workflowList, workflowReload, type Workflow } from "../../ipc/workflow";

export default function WorkflowsPanel() {
  const [workflows, setWorkflows] = useState<Workflow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [selectedName, setSelectedName] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await workflowList();
      setWorkflows(list);
      if (list.length > 0 && !selectedName) {
        setSelectedName(list[0].workflow.name);
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [selectedName]);

  useEffect(() => {
    void load();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      await workflowReload();
      const list = await workflowList();
      setWorkflows(list);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  const selected = useMemo(
    () =>
      workflows.find((w) => w.workflow.name === selectedName) ?? null,
    [workflows, selectedName],
  );

  return (
    <div
      style={{
        display: "flex",
        height: "100%",
        fontSize: 13,
        color: "var(--color-fg)",
      }}
    >
      <div
        style={{
          width: 240,
          minWidth: 180,
          borderRight: "1px solid var(--color-border)",
          display: "flex",
          flexDirection: "column",
        }}
      >
        <div
          style={{
            padding: "6px 10px",
            borderBottom: "1px solid var(--color-border)",
            display: "flex",
            gap: 6,
            alignItems: "center",
          }}
        >
          <span style={{ flex: 1, opacity: 0.75 }}>
            Workflows ({workflows.length})
          </span>
          <button
            type="button"
            onClick={reload}
            disabled={loading}
            style={chipButtonStyle}
          >
            Reload
          </button>
        </div>
        <div style={{ overflowY: "auto", flex: 1 }}>
          {workflows.length === 0 && !loading ? (
            <div style={{ padding: 10, opacity: 0.6 }}>
              No workflows in <code>.workflows/</code>.
            </div>
          ) : null}
          {workflows.map((w) => (
            <button
              key={w.workflow.name}
              type="button"
              onClick={() => setSelectedName(w.workflow.name)}
              aria-pressed={w.workflow.name === selectedName}
              style={{
                display: "block",
                width: "100%",
                textAlign: "left",
                padding: "6px 10px",
                background:
                  w.workflow.name === selectedName
                    ? "var(--color-accent-bg, rgba(255,255,255,0.05))"
                    : "transparent",
                border: "none",
                borderBottom: "1px solid var(--color-border)",
                color: "inherit",
                cursor: "pointer",
                fontSize: 13,
              }}
            >
              <div style={{ fontWeight: 500 }}>{w.workflow.name}</div>
              <div style={{ opacity: 0.6, fontSize: 11 }}>
                trigger: {w.trigger.type}
              </div>
            </button>
          ))}
        </div>
      </div>
      <div style={{ flex: 1, display: "flex", flexDirection: "column" }}>
        {error && (
          <div
            role="alert"
            style={{
              padding: "6px 10px",
              background: "var(--color-error-bg, #4a1f1f)",
              color: "var(--color-error-fg, #ffd)",
              fontSize: 12,
            }}
          >
            {error}
          </div>
        )}
        {selected ? (
          <WorkflowDetail workflow={selected} />
        ) : (
          <div style={{ padding: 20, opacity: 0.6 }}>
            Select a workflow to view its pipeline.
          </div>
        )}
      </div>
    </div>
  );
}

function WorkflowDetail({ workflow }: { workflow: Workflow }) {
  const meta = workflow.workflow;
  return (
    <div style={{ overflowY: "auto", padding: "10px 14px" }}>
      <h2 style={{ margin: "0 0 4px 0", fontSize: 16 }}>{meta.name}</h2>
      <div style={{ opacity: 0.7, fontSize: 12, marginBottom: 8 }}>
        {meta.version ? `v${meta.version}` : ""}
        {meta.author ? ` · by ${meta.author}` : ""}
      </div>
      {meta.description && (
        <p style={{ margin: "0 0 10px 0" }}>{meta.description}</p>
      )}
      <div style={{ fontSize: 12, margin: "10px 0" }}>
        <div style={{ opacity: 0.7 }}>Trigger</div>
        <div
          style={{
            padding: 8,
            marginTop: 2,
            background: "var(--color-bg-subtle, rgba(255,255,255,0.03))",
            borderRadius: 4,
            fontFamily: "var(--font-mono)",
          }}
        >
          <code>{workflow.trigger.type}</code>
          {Object.entries(workflow.trigger)
            .filter(([k]) => k !== "type")
            .map(([k, v]) => (
              <div key={k} style={{ fontSize: 11, marginTop: 2 }}>
                <span style={{ opacity: 0.7 }}>{k} = </span>
                <span>{JSON.stringify(v)}</span>
              </div>
            ))}
        </div>
      </div>
      {workflow.condition && (
        <div style={{ fontSize: 12, margin: "10px 0" }}>
          <div style={{ opacity: 0.7 }}>Condition</div>
          <div
            style={{
              padding: 8,
              marginTop: 2,
              background: "var(--color-bg-subtle, rgba(255,255,255,0.03))",
              borderRadius: 4,
              fontFamily: "var(--font-mono)",
              fontSize: 11,
            }}
          >
            <code>{workflow.condition.type}</code>
          </div>
        </div>
      )}
      {workflow.steps && workflow.steps.length > 0 && (
        <div style={{ fontSize: 12, margin: "10px 0" }}>
          <div style={{ opacity: 0.7 }}>Steps ({workflow.steps.length})</div>
          <ol style={{ margin: "4px 0", paddingLeft: 20 }}>
            {workflow.steps.map((step, i) => (
              <li key={step.name ?? `${step.type}-${i}`}>
                <code>{step.type}</code>
                {step.name ? (
                  <span style={{ opacity: 0.7 }}> — {step.name}</span>
                ) : null}
                {step.parallel ? (
                  <span style={{ opacity: 0.7 }}> [parallel]</span>
                ) : null}
              </li>
            ))}
          </ol>
        </div>
      )}
    </div>
  );
}

const chipButtonStyle: React.CSSProperties = {
  padding: "2px 8px",
  borderRadius: 4,
  border: "1px solid var(--color-border)",
  background: "transparent",
  color: "inherit",
  fontSize: 11,
  cursor: "pointer",
};
