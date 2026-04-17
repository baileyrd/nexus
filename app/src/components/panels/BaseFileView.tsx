// Editable `.bases` directory surface.
//
// Loads the base off disk via `load_forge_base`, renders any
// configured view through `BaseViewPanel`, and exposes a lightweight
// editing UX — inline cell editing, add/delete record, switch the
// active view. All mutations round-trip through
// `save_forge_base`, debounced so rapid typing doesn't thrash the
// disk.
//
// The view engine itself (filter/sort/group) stays in the Rust layer
// behind `com.nexus.database::apply_view`. This component only owns:
//
// - local record state (the source of truth while the tab is open)
// - the "which view is active" UI state
// - a debounced save pipe
//
// Related renderers: `./BaseView.tsx` (read-only, any AppliedView).
// Related ipc: `../../ipc/database.ts` (loadBase, saveBase).

import type { ReactNode } from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  loadBase,
  saveBase,
  type BaseRecord,
  type BaseView,
  type LoadedBase,
} from "../../ipc/database";
import { BaseViewPanel } from "./BaseView";

interface BaseFileViewProps {
  /** Forge-relative path to the `.bases` directory. */
  relpath: string;
}

/** Delay between the last edit and the autosave flush. Short enough
 *  that Ctrl-closing the tab rarely loses data; long enough to batch
 *  a multi-field edit into one write. */
const SAVE_DEBOUNCE_MS = 400;

type LoadState =
  | { kind: "loading" }
  | { kind: "ready"; base: LoadedBase }
  | { kind: "error"; message: string };

type SaveStatus = "idle" | "saving" | "error";

export function BaseFileView({ relpath }: BaseFileViewProps) {
  const [state, setState] = useState<LoadState>({ kind: "loading" });
  const [activeViewIdx, setActiveViewIdx] = useState(0);
  const [saveStatus, setSaveStatus] = useState<SaveStatus>("idle");
  const saveTimerRef = useRef<number | null>(null);
  const latestBaseRef = useRef<LoadedBase | null>(null);

  // Load on mount + whenever relpath flips. Resets view selection so
  // a fresh tab doesn't land on a stale view index.
  useEffect(() => {
    let cancelled = false;
    setState({ kind: "loading" });
    setActiveViewIdx(0);
    loadBase(relpath).then(
      (base) => {
        if (cancelled) return;
        latestBaseRef.current = base;
        setState({ kind: "ready", base });
      },
      (err) => {
        if (!cancelled) setState({ kind: "error", message: String(err) });
      },
    );
    return () => {
      cancelled = true;
    };
  }, [relpath]);

  // Schedule a debounced save. Called after any local mutation.
  const scheduleSave = useCallback(
    (next: LoadedBase) => {
      latestBaseRef.current = next;
      if (saveTimerRef.current != null) {
        window.clearTimeout(saveTimerRef.current);
      }
      saveTimerRef.current = window.setTimeout(() => {
        const payload = latestBaseRef.current;
        if (!payload) return;
        setSaveStatus("saving");
        saveBase(relpath, payload).then(
          () => setSaveStatus("idle"),
          () => setSaveStatus("error"),
        );
      }, SAVE_DEBOUNCE_MS);
    },
    [relpath],
  );

  // Flush pending save on unmount / relpath change so a fast
  // tab-close doesn't drop the last edit.
  useEffect(() => {
    return () => {
      if (saveTimerRef.current != null) {
        window.clearTimeout(saveTimerRef.current);
        const payload = latestBaseRef.current;
        if (payload) {
          void saveBase(relpath, payload);
        }
      }
    };
  }, [relpath]);

  const applyRecordUpdate = useCallback(
    (updater: (records: BaseRecord[]) => BaseRecord[]) => {
      setState((prev) => {
        if (prev.kind !== "ready") return prev;
        const next: LoadedBase = {
          ...prev.base,
          records: updater(prev.base.records),
        };
        scheduleSave(next);
        return { kind: "ready", base: next };
      });
    },
    [scheduleSave],
  );

  if (state.kind === "loading") {
    return (
      <div className="base-file-view loading">
        <p>Loading {relpath}…</p>
      </div>
    );
  }
  if (state.kind === "error") {
    return (
      <div className="base-file-view error" role="alert">
        <p className="label">Failed to load base</p>
        <p className="message">{state.message}</p>
        <p className="hint">{relpath}</p>
      </div>
    );
  }

  const { base } = state;
  // Empty views list → synthesize a default table view so the base is
  // at least inspectable on first load.
  const views: BaseView[] = base.views.length > 0
    ? base.views
    : [defaultTableView(base)];
  const activeView = views[Math.min(activeViewIdx, views.length - 1)];

  return (
    <div className="base-file-view">
      <Toolbar
        name={base.name}
        relpath={relpath}
        views={views}
        activeViewIdx={activeViewIdx}
        onChangeView={setActiveViewIdx}
        saveStatus={saveStatus}
        onAddRecord={() => applyRecordUpdate(addBlankRecord(base))}
      />
      <EditableRecords base={base} onUpdate={applyRecordUpdate} />
      <div className="base-file-section-divider">Active view</div>
      <div className="base-file-view-body">
        <BaseViewPanel
          records={base.records}
          schema={base.schema}
          view={activeView}
        />
      </div>
    </div>
  );
}

// ── Toolbar ─────────────────────────────────────────────────────────────────

function Toolbar(props: {
  name: string;
  relpath: string;
  views: BaseView[];
  activeViewIdx: number;
  onChangeView: (idx: number) => void;
  saveStatus: SaveStatus;
  onAddRecord: () => void;
}) {
  const { name, relpath, views, activeViewIdx, onChangeView, saveStatus, onAddRecord } =
    props;
  return (
    <header className="base-file-view-toolbar">
      <div className="base-file-identity">
        <strong>{name}</strong>
        <span className="relpath">{relpath}</span>
      </div>
      <div className="base-file-view-tabs">
        {views.map((v, idx) => (
          <button
            key={v.name + idx}
            type="button"
            className={idx === activeViewIdx ? "tab active" : "tab"}
            onClick={() => onChangeView(idx)}
            title={v.type}
          >
            {v.name}
          </button>
        ))}
      </div>
      <div className="base-file-view-actions">
        <span
          className={`save-indicator ${saveStatus}`}
          title={
            saveStatus === "saving"
              ? "Saving…"
              : saveStatus === "error"
                ? "Save failed — edit again to retry"
                : "All changes saved"
          }
        >
          {saveStatus === "saving" && "Saving…"}
          {saveStatus === "error" && "Save failed"}
          {saveStatus === "idle" && "Saved"}
        </span>
        <button type="button" className="add-record" onClick={onAddRecord}>
          + Record
        </button>
      </div>
    </header>
  );
}

// ── Editable records ────────────────────────────────────────────────────────

function EditableRecords({
  base,
  onUpdate,
}: {
  base: LoadedBase;
  onUpdate: (updater: (records: BaseRecord[]) => BaseRecord[]) => void;
}) {
  const columns = useMemo(() => inferColumns(base), [base]);

  return (
    <div className="base-file-records">
      <table>
        <thead>
          <tr>
            <th className="col-id">id</th>
            {columns.map((c) => (
              <th key={c}>{c}</th>
            ))}
            <th className="col-actions" aria-label="row actions" />
          </tr>
        </thead>
        <tbody>
          {base.records.map((r) => (
            <EditableRow
              key={r.id}
              record={r}
              columns={columns}
              onEdit={(field, value) => {
                onUpdate((records) =>
                  records.map((rec) =>
                    rec.id === r.id ? { ...rec, [field]: value } : rec,
                  ),
                );
              }}
              onDelete={() => {
                onUpdate((records) => records.filter((rec) => rec.id !== r.id));
              }}
            />
          ))}
          {base.records.length === 0 && (
            <tr>
              <td colSpan={columns.length + 2} className="empty-row">
                No records yet. Click “+ Record” to add one.
              </td>
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}

function EditableRow({
  record,
  columns,
  onEdit,
  onDelete,
}: {
  record: BaseRecord;
  columns: string[];
  onEdit: (field: string, value: unknown) => void;
  onDelete: () => void;
}) {
  return (
    <tr>
      <td className="col-id" title={record.id}>
        {shortId(record.id)}
      </td>
      {columns.map((col) => (
        <EditableCell
          key={col}
          value={record[col]}
          onCommit={(v) => onEdit(col, v)}
        />
      ))}
      <td className="col-actions">
        <button
          type="button"
          className="row-delete"
          onClick={onDelete}
          aria-label="delete row"
          title="Delete record"
        >
          ×
        </button>
      </td>
    </tr>
  );
}

function EditableCell({
  value,
  onCommit,
}: {
  value: unknown;
  onCommit: (value: unknown) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState<string>(stringifyCell(value));

  useEffect(() => {
    if (!editing) setDraft(stringifyCell(value));
  }, [value, editing]);

  const commit = useCallback(() => {
    setEditing(false);
    const parsed = parseCell(draft);
    // Only fire onCommit if the value actually changed so a
    // click-to-select flow doesn't mark the base as dirty.
    if (!cellsEqual(parsed, value)) {
      onCommit(parsed);
    }
  }, [draft, onCommit, value]);

  if (editing) {
    return (
      <td className="cell editing">
        <input
          autoFocus
          className="cell-input"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              commit();
            } else if (e.key === "Escape") {
              e.preventDefault();
              setDraft(stringifyCell(value));
              setEditing(false);
            }
          }}
        />
      </td>
    );
  }
  return (
    <td className="cell" onClick={() => setEditing(true)}>
      {displayCell(value)}
    </td>
  );
}

// ── Pure helpers ────────────────────────────────────────────────────────────

function inferColumns(base: LoadedBase): string[] {
  const schemaFields = Object.keys(base.schema.fields ?? {}).filter(
    (k) => k !== "id",
  );
  if (schemaFields.length > 0) return schemaFields;
  // Fallback: union of keys across records, stable by insertion.
  const seen = new Set<string>();
  for (const r of base.records) {
    for (const k of Object.keys(r)) {
      if (k !== "id") seen.add(k);
    }
  }
  return Array.from(seen);
}

function defaultTableView(base: LoadedBase): BaseView {
  return {
    name: "All records",
    type: "table",
    fields: inferColumns(base),
    sort: [],
    filter: [],
  };
}

function addBlankRecord(base: LoadedBase) {
  return (records: BaseRecord[]): BaseRecord[] => {
    const id = `r${Date.now()}${Math.random().toString(36).slice(2, 6)}`;
    const blank: BaseRecord = { id };
    for (const col of inferColumns(base)) {
      blank[col] = null;
    }
    return [...records, blank];
  };
}

/** Convert a JSON-ish value into an editable string form. Arrays
 *  become comma-separated; booleans round-trip via `true`/`false`;
 *  nulls become empty; objects JSON-stringify. */
function stringifyCell(value: unknown): string {
  if (value === null || value === undefined) return "";
  if (typeof value === "boolean") return value ? "true" : "false";
  if (Array.isArray(value)) return value.map((v) => stringifyCell(v)).join(", ");
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

/** Inverse of [`stringifyCell`] — applies cheap heuristics so a user
 *  editing `true`, `42`, or `a, b, c` gets the natural JSON shape
 *  back without needing a type selector. Anything that doesn't match
 *  a heuristic passes through as a plain string. */
function parseCell(input: string): unknown {
  const trimmed = input.trim();
  if (trimmed === "") return null;
  if (trimmed === "true") return true;
  if (trimmed === "false") return false;
  if (trimmed === "null") return null;
  if (/^-?\d+$/.test(trimmed)) {
    const n = Number(trimmed);
    if (Number.isFinite(n)) return n;
  }
  if (/^-?\d*\.\d+$/.test(trimmed)) {
    const n = Number(trimmed);
    if (Number.isFinite(n)) return n;
  }
  // JSON literals (objects/arrays) round-trip. If parsing fails, fall
  // through to string — a user typing a bare word shouldn't see an
  // error because their text happens to start with `{`.
  if (trimmed.startsWith("{") || trimmed.startsWith("[")) {
    try {
      return JSON.parse(trimmed);
    } catch {
      /* fall through */
    }
  }
  // Heuristic: anything with a comma becomes a string array so
  // multi-select-style entries round-trip without a dedicated editor.
  if (trimmed.includes(",")) {
    return trimmed
      .split(",")
      .map((s) => s.trim())
      .filter((s) => s.length > 0);
  }
  return trimmed;
}

function cellsEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (a === null || b === null || a === undefined || b === undefined) {
    return (a ?? null) === (b ?? null);
  }
  if (Array.isArray(a) && Array.isArray(b)) {
    if (a.length !== b.length) return false;
    return a.every((v, i) => cellsEqual(v, b[i]));
  }
  if (typeof a === "object" && typeof b === "object") {
    return JSON.stringify(a) === JSON.stringify(b);
  }
  return false;
}

function displayCell(value: unknown): ReactNode {
  if (value === null || value === undefined) {
    return <span className="cell-null">—</span>;
  }
  if (typeof value === "boolean") return value ? "✓" : "·";
  if (Array.isArray(value)) {
    return (
      <span className="cell-tags">
        {value.map((v, i) => (
          <span key={i} className="cell-tag">
            {stringifyCell(v)}
          </span>
        ))}
      </span>
    );
  }
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

function shortId(id: string): string {
  return id.length > 8 ? `${id.slice(0, 8)}…` : id;
}
