// Agent history browser panel (PRD-15).
//
// Two-pane read-only view over `com.nexus.agent::history_list` /
// `::history_get`: left column lists every persisted run (newest
// first); right column shows the full plan + observation. "Reload"
// re-reads the archive; "Delete" removes the selected entry.

import { useCallback, useEffect, useMemo, useState } from "react";

import {
  agentHistoryDelete,
  agentHistoryGet,
  agentHistoryList,
  type AgentHistoryEntry,
  type AgentHistoryRecord,
  type StepResult,
} from "../../ipc/agent";

export default function AgentHistoryPanel() {
  const [entries, setEntries] = useState<AgentHistoryEntry[]>([]);
  const [selected, setSelected] = useState<AgentHistoryRecord | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await agentHistoryList();
      // Sort newest first by created_at (timestamps are ts-<secs>).
      list.sort((a, b) =>
        (b.created_at ?? "").localeCompare(a.created_at ?? ""),
      );
      setEntries(list);
      if (list.length > 0 && !selectedId) {
        setSelectedId(list[0].plan_id);
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [selectedId]);

  useEffect(() => {
    void reload();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!selectedId) {
      setSelected(null);
      return;
    }
    let cancelled = false;
    agentHistoryGet(selectedId)
      .then((record) => {
        if (!cancelled) setSelected(record);
      })
      .catch((err) => {
        if (!cancelled) setError(String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [selectedId]);

  const deleteSelected = useCallback(async () => {
    if (!selectedId) return;
    try {
      await agentHistoryDelete(selectedId);
    } catch (err) {
      setError(String(err));
      return;
    }
    setSelectedId(null);
    setSelected(null);
    await reload();
  }, [selectedId, reload]);

  const headerStats = useMemo(() => {
    const ok = entries.filter((e) => e.success === true).length;
    const failed = entries.filter((e) => e.success === false).length;
    return `${entries.length} total · ${ok} ok · ${failed} failed`;
  }, [entries]);

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
          width: 280,
          minWidth: 220,
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
          <span style={{ flex: 1, opacity: 0.75, fontSize: 11 }}>
            {headerStats}
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
          {entries.length === 0 && !loading ? (
            <div style={{ padding: 10, opacity: 0.6 }}>
              No runs yet. Plans are archived after every agent run.
            </div>
          ) : null}
          {entries.map((e) => (
            <button
              key={e.plan_id}
              type="button"
              onClick={() => setSelectedId(e.plan_id)}
              aria-pressed={e.plan_id === selectedId}
              style={{
                display: "block",
                width: "100%",
                textAlign: "left",
                padding: "6px 10px",
                background:
                  e.plan_id === selectedId
                    ? "var(--color-accent-bg, rgba(255,255,255,0.05))"
                    : "transparent",
                border: "none",
                borderBottom: "1px solid var(--color-border)",
                color: "inherit",
                cursor: "pointer",
                fontSize: 13,
              }}
            >
              <div
                style={{
                  fontWeight: 500,
                  display: "flex",
                  gap: 6,
                  alignItems: "baseline",
                }}
              >
                <span>{statusBadge(e.success)}</span>
                <span
                  style={{
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {e.goal ?? e.plan_id}
                </span>
              </div>
              <div style={{ opacity: 0.6, fontSize: 11 }}>
                {e.steps} step{e.steps === 1 ? "" : "s"} · {e.plan_id}
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
          <HistoryDetail record={selected} onDelete={deleteSelected} />
        ) : (
          <div style={{ padding: 20, opacity: 0.6 }}>
            Select a run to view its plan + observation.
          </div>
        )}
      </div>
    </div>
  );
}

function HistoryDetail({
  record,
  onDelete,
}: {
  record: AgentHistoryRecord;
  onDelete: () => void;
}) {
  const resultsById = new Map<string, StepResult>(
    record.observation.steps.map((s) => [s.step_id, s]),
  );
  return (
    <div style={{ overflowY: "auto", padding: "10px 14px" }}>
      <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
        <h2 style={{ margin: "0 0 4px 0", fontSize: 16, flex: 1 }}>
          {record.goal ?? record.plan_id}
        </h2>
        <button type="button" onClick={onDelete} style={chipButtonStyle}>
          Delete
        </button>
      </div>
      <div style={{ opacity: 0.7, fontSize: 12, marginBottom: 8 }}>
        {record.plan_id} · {record.created_at ?? "unknown time"} ·{" "}
        {record.observation.success ? "success" : "partial / failed"}
      </div>
      <ol
        style={{
          margin: 0,
          paddingInlineStart: 20,
          fontSize: 13,
          lineHeight: 1.4,
        }}
      >
        {record.plan.steps.map((step) => {
          const result = resultsById.get(step.id);
          return (
            <li key={step.id} style={{ marginBottom: 8 }}>
              <div>
                {result ? stepBadge(result.status) : ""}
                {step.description}
              </div>
              {step.tool_call && (
                <div
                  style={{
                    fontFamily: "var(--font-mono, monospace)",
                    fontSize: 11,
                    opacity: 0.7,
                  }}
                >
                  → {step.tool_call.target_plugin_id}.{step.tool_call.command_id}
                </div>
              )}
              {result && result.response ? (
                <pre
                  style={{
                    margin: "4px 0",
                    padding: 8,
                    background:
                      "var(--color-bg-subtle, rgba(255,255,255,0.03))",
                    borderRadius: 4,
                    whiteSpace: "pre-wrap",
                    fontSize: 11,
                  }}
                >
                  {previewJson(result.response)}
                </pre>
              ) : null}
            </li>
          );
        })}
      </ol>
    </div>
  );
}

function statusBadge(success?: boolean | null): string {
  if (success === true) return "✓";
  if (success === false) return "✗";
  return "·";
}

function stepBadge(status: StepResult["status"]): string {
  switch (status) {
    case "ok":
      return "✓ ";
    case "denied":
      return "⊘ ";
    case "failed":
      return "✗ ";
    case "skipped":
      return "· ";
    default:
      return "";
  }
}

function previewJson(value: unknown, max = 400): string {
  const text =
    typeof value === "string" ? value : JSON.stringify(value, null, 2);
  if (text.length <= max) return text;
  return `${text.slice(0, max)}…`;
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
