// PRD-10 §4 view renderers.
//
// One dispatcher + four renderers (Table/Kanban/Calendar/Gallery).
// The `apply_view` engine in `com.nexus.database` returns a
// pre-filtered/sorted/grouped `AppliedView`; the components below
// only deal with layout + cell rendering, never filter/sort logic.
//
// Each renderer takes the `AppliedView` produced by
// `invoke("db_apply_view", …)`. The dispatcher picks the right one
// based on `view_type`. Renderers fall back to a flat-list view when
// a grouped-layout view was configured without a grouping field.

import { useEffect, useMemo, useState } from "react";

import {
  applyView,
  MISSING_GROUP_KEY,
  type AppliedView,
  type BaseRecord,
  type BaseSchema,
  type BaseView,
  type ViewGroup,
} from "../../ipc/database";

// ── Public surface ───────────────────────────────────────────────────────────

/**
 * Load + render a base view. Calls `apply_view` on mount (and when
 * any of the props change) and renders the resulting shape. Works
 * for any of the four view types — the dispatcher at the bottom
 * picks the right sub-renderer.
 */
export function BaseViewPanel(props: {
  records: BaseRecord[];
  schema: BaseSchema;
  view: BaseView;
}) {
  const { records, schema, view } = props;
  const [applied, setApplied] = useState<AppliedView | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setError(null);
    applyView(records, schema, view).then(
      (result) => {
        if (!cancelled) setApplied(result);
      },
      (err) => {
        if (!cancelled) setError(String(err));
      },
    );
    return () => {
      cancelled = true;
    };
  }, [records, schema, view]);

  if (error) {
    return (
      <div className="base-view error" role="alert">
        <p className="label">Failed to apply view</p>
        <p className="message">{error}</p>
      </div>
    );
  }
  if (!applied) {
    return (
      <div className="base-view loading">
        <p>Loading view…</p>
      </div>
    );
  }
  return <AppliedViewRenderer applied={applied} />;
}

/** Renders an already-applied view without re-running the engine.
 *  Useful when a caller (e.g. a plugin) has pre-computed the layout
 *  and wants to swap in a different renderer. */
export function AppliedViewRenderer({ applied }: { applied: AppliedView }) {
  switch (applied.view_type) {
    case "kanban":
      return <KanbanView applied={applied} />;
    case "calendar":
      return <CalendarView applied={applied} />;
    case "gallery":
      return <GalleryView applied={applied} />;
    case "table":
    default:
      return <TableView applied={applied} />;
  }
}

// ── Renderers ────────────────────────────────────────────────────────────────

/** Cell renderer — stringifies any JSON value into a compact display
 *  form. Arrays join with `, ` for multi-select pills; booleans become
 *  ✓/·; nulls render as a greyed placeholder. Rich typed rendering
 *  (dates, relations) lands with follow-up property-type work. */
function cellValue(value: unknown): string {
  if (value === null || value === undefined) return "—";
  if (typeof value === "boolean") return value ? "✓" : "·";
  if (Array.isArray(value)) return value.map((v) => cellValue(v)).join(", ");
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

/** Best-effort list of columns to show — prefer the view's declared
 *  fields; fall back to the union of keys across all records when the
 *  view didn't specify any. */
function resolveColumns(applied: AppliedView, records: BaseRecord[]): string[] {
  if (applied.fields.length > 0) return applied.fields;
  const seen = new Set<string>();
  for (const r of records) {
    for (const k of Object.keys(r)) {
      if (k !== "id") seen.add(k);
    }
  }
  return Array.from(seen);
}

function flatRecords(applied: AppliedView): BaseRecord[] {
  return applied.layout.kind === "flat"
    ? applied.layout.records
    : applied.layout.groups.flatMap((g) => g.records);
}

function TableView({ applied }: { applied: AppliedView }) {
  const records = flatRecords(applied);
  const cols = useMemo(() => resolveColumns(applied, records), [applied, records]);

  if (records.length === 0) {
    return <EmptyView name={applied.view_name} />;
  }
  return (
    <div className="base-view base-view-table">
      <table>
        <thead>
          <tr>
            {cols.map((c) => (
              <th key={c}>{c}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {records.map((r) => (
            <tr key={r.id}>
              {cols.map((c) => (
                <td key={c}>{cellValue(r[c])}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function GalleryView({ applied }: { applied: AppliedView }) {
  const records = flatRecords(applied);
  const cols = useMemo(() => resolveColumns(applied, records), [applied, records]);

  if (records.length === 0) {
    return <EmptyView name={applied.view_name} />;
  }
  // First column becomes the card title; the rest render as small
  // key/value pairs below it. Deliberately unopinionated so any
  // schema shape looks at least passable.
  const [titleCol, ...rest] = cols;
  return (
    <div className="base-view base-view-gallery">
      {records.map((r) => (
        <div className="base-card" key={r.id}>
          <div className="base-card-title">
            {titleCol ? cellValue(r[titleCol]) : r.id}
          </div>
          {rest.length > 0 && (
            <dl className="base-card-fields">
              {rest.map((c) => (
                <div key={c} className="base-card-field">
                  <dt>{c}</dt>
                  <dd>{cellValue(r[c])}</dd>
                </div>
              ))}
            </dl>
          )}
        </div>
      ))}
    </div>
  );
}

function KanbanView({ applied }: { applied: AppliedView }) {
  if (applied.layout.kind !== "grouped") {
    // Kanban configured without a `groupField` — the engine returned
    // a flat layout. Fall through to TableView rather than pretending
    // to show a single-column board.
    return <TableView applied={applied} />;
  }
  const groups = applied.layout.groups;
  if (groups.every((g) => g.records.length === 0)) {
    return <EmptyView name={applied.view_name} />;
  }
  return (
    <div className="base-view base-view-kanban">
      {groups.map((g) => (
        <KanbanColumn key={g.key} group={g} fields={applied.fields} />
      ))}
    </div>
  );
}

function KanbanColumn({ group, fields }: { group: ViewGroup; fields: string[] }) {
  const title = group.key === MISSING_GROUP_KEY ? "Uncategorised" : group.key;
  const cols = useMemo(
    () => (fields.length > 0 ? fields : inferColumns(group.records)),
    [fields, group.records],
  );
  return (
    <div className="kanban-column">
      <header className="kanban-column-header">
        <span className="kanban-column-title">{title}</span>
        <span className="kanban-column-count">{group.records.length}</span>
      </header>
      <div className="kanban-cards">
        {group.records.map((r) => (
          <div className="kanban-card" key={r.id}>
            {cols.map((c) => (
              <div key={c} className="kanban-card-row">
                <span className="kanban-card-key">{c}</span>
                <span className="kanban-card-value">{cellValue(r[c])}</span>
              </div>
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}

function CalendarView({ applied }: { applied: AppliedView }) {
  if (applied.layout.kind !== "grouped") {
    return <TableView applied={applied} />;
  }
  const groups = applied.layout.groups;
  if (groups.every((g) => g.records.length === 0)) {
    return <EmptyView name={applied.view_name} />;
  }
  // Calendar layout is a vertical day-by-day list. A proper month
  // grid with week rows is a richer component and can layer on top;
  // this is the minimum useful shape.
  return (
    <div className="base-view base-view-calendar">
      {groups.map((g) => (
        <section key={g.key} className="calendar-day">
          <header className="calendar-day-header">
            {g.key === MISSING_GROUP_KEY ? "No date" : g.key}
            <span className="calendar-day-count">{g.records.length}</span>
          </header>
          <ul className="calendar-events">
            {g.records.map((r) => (
              <li key={r.id} className="calendar-event">
                {cellValue(r[applied.fields[0] ?? "id"])}
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}

function EmptyView({ name }: { name: string }) {
  return (
    <div className="base-view empty">
      <p className="label">{name}</p>
      <p className="hint">No records match this view.</p>
    </div>
  );
}

function inferColumns(records: BaseRecord[]): string[] {
  const seen = new Set<string>();
  for (const r of records) {
    for (const k of Object.keys(r)) {
      if (k !== "id") seen.add(k);
    }
  }
  return Array.from(seen);
}
