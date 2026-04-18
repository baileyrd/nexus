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
        {group.records.map((r) => {
          const [titleCol, ...rest] = cols;
          return (
            <div className="kanban-card" key={r.id}>
              {titleCol && (
                <div className="kanban-card-title">{cellValue(r[titleCol])}</div>
              )}
              {rest.map((c) => (
                <div key={c} className="kanban-card-row">
                  <span className="kanban-card-key">{c}</span>
                  <span className="kanban-card-value">{cellValue(r[c])}</span>
                </div>
              ))}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function CalendarView({ applied }: { applied: AppliedView }) {
  if (applied.layout.kind !== "grouped") {
    return <TableView applied={applied} />;
  }
  const groups = applied.layout.groups;

  // Pick the first group with a valid ISO date as the initial month
  // anchor. Falls back to today if nothing matches (e.g. every record
  // landed in MISSING_GROUP_KEY).
  const firstDated = groups.find(
    (g) => g.key !== MISSING_GROUP_KEY && /^\d{4}-\d{2}-\d{2}/.test(g.key),
  );
  const anchor = firstDated
    ? parseIsoDay(firstDated.key) ?? startOfToday()
    : startOfToday();

  const [month, setMonth] = useState<{ year: number; month: number }>({
    year: anchor.getFullYear(),
    month: anchor.getMonth(),
  });

  const byDay = useMemo(() => {
    const map = new Map<string, ViewGroup["records"]>();
    for (const g of groups) {
      if (g.key === MISSING_GROUP_KEY) continue;
      // Accept full ISO timestamps too — bucket by the YYYY-MM-DD prefix.
      const key = g.key.slice(0, 10);
      const existing = map.get(key) ?? [];
      map.set(key, existing.concat(g.records));
    }
    return map;
  }, [groups]);

  const undated = useMemo(
    () => groups.find((g) => g.key === MISSING_GROUP_KEY)?.records ?? [],
    [groups],
  );

  if (groups.every((g) => g.records.length === 0)) {
    return <EmptyView name={applied.view_name} />;
  }

  const cells = monthCells(month.year, month.month);
  const titleField = applied.fields[0] ?? "id";
  const monthLabel = new Date(month.year, month.month, 1).toLocaleString(
    undefined,
    { month: "long", year: "numeric" },
  );
  const weekdayLabels = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

  const shift = (delta: number) =>
    setMonth((m) => {
      const next = new Date(m.year, m.month + delta, 1);
      return { year: next.getFullYear(), month: next.getMonth() };
    });

  return (
    <div className="base-view base-view-calendar-month">
      <header className="calendar-month-header">
        <button
          type="button"
          className="calendar-month-nav"
          onClick={() => shift(-1)}
          aria-label="Previous month"
        >
          ‹
        </button>
        <span className="calendar-month-title">{monthLabel}</span>
        <button
          type="button"
          className="calendar-month-nav"
          onClick={() => shift(1)}
          aria-label="Next month"
        >
          ›
        </button>
      </header>
      <div className="calendar-month-weekdays">
        {weekdayLabels.map((w) => (
          <div key={w} className="calendar-month-weekday">
            {w}
          </div>
        ))}
      </div>
      <div className="calendar-month-grid">
        {cells.map((cell) => {
          const key = isoDay(cell.date);
          const records = byDay.get(key) ?? [];
          const inMonth = cell.date.getMonth() === month.month;
          return (
            <div
              key={key}
              className={
                "calendar-month-cell" + (inMonth ? "" : " out-of-month")
              }
            >
              <div className="calendar-month-daynum">{cell.date.getDate()}</div>
              <ul className="calendar-month-events">
                {records.map((r) => (
                  <li key={r.id} className="calendar-event" title={String(r.id)}>
                    {cellValue(r[titleField])}
                  </li>
                ))}
              </ul>
            </div>
          );
        })}
      </div>
      {undated.length > 0 && (
        <section className="calendar-month-undated">
          <header className="calendar-day-header">
            No date
            <span className="calendar-day-count">{undated.length}</span>
          </header>
          <ul className="calendar-events">
            {undated.map((r) => (
              <li key={r.id} className="calendar-event">
                {cellValue(r[titleField])}
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}

function parseIsoDay(key: string): Date | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(key);
  if (!m) return null;
  const d = new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]));
  return Number.isNaN(d.getTime()) ? null : d;
}

function startOfToday(): Date {
  const now = new Date();
  return new Date(now.getFullYear(), now.getMonth(), now.getDate());
}

function isoDay(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/** Six-row Sun-anchored grid covering the given month. */
function monthCells(year: number, month: number): { date: Date }[] {
  const first = new Date(year, month, 1);
  const start = new Date(first);
  start.setDate(first.getDate() - first.getDay());
  const cells: { date: Date }[] = [];
  for (let i = 0; i < 42; i++) {
    const d = new Date(start);
    d.setDate(start.getDate() + i);
    cells.push({ date: d });
  }
  return cells;
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
