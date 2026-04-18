// Demo surface for PRD-10 view renderers.
//
// Mounts a hardcoded sample `.bases`-style dataset (tasks with
// status/priority/due) and lets the user flip between the four view
// types registered in `nexus_types::bases::ViewType`. Confirms the
// end-to-end `invoke("db_apply_view", …)` loop and exercises the
// TableView / KanbanView / CalendarView / GalleryView renderers
// against a realistic record shape.
//
// Replaced by real `.bases` tab integration once a file handler for
// `.bases` directories lands.

import { useCallback, useState } from "react";
import { BaseViewPanel } from "./BaseView";
import type { BaseRecord, BaseSchema, BaseView, ViewType } from "../../ipc/database";

const SCHEMA: BaseSchema = {
  version: "1.0",
  fields: {
    title: { type: "text", required: true },
    status: { type: "select", options: ["todo", "in-progress", "done"] },
    priority: { type: "number", min: 1, max: 5 },
    due: { type: "date" },
    tags: { type: "multi-select", options: ["backend", "frontend", "docs"] },
  },
};

const RECORDS: BaseRecord[] = [
  {
    id: "t1",
    title: "Wire terminal panel into Tauri",
    status: "done",
    priority: 5,
    due: "2026-04-17",
    tags: ["frontend"],
  },
  {
    id: "t2",
    title: "Ship view engine",
    status: "done",
    priority: 4,
    due: "2026-04-17",
    tags: ["backend"],
  },
  {
    id: "t3",
    title: "Build view renderers",
    status: "in-progress",
    priority: 4,
    due: "2026-04-18",
    tags: ["frontend"],
  },
  {
    id: "t4",
    title: "Write docs for Base schema",
    status: "todo",
    priority: 2,
    due: "2026-04-20",
    tags: ["docs"],
  },
  {
    id: "t5",
    title: "Add rollup support",
    status: "todo",
    priority: 3,
    due: null,
    tags: ["backend"],
  },
  {
    id: "t6",
    title: "Plan PRD-15 agent slice",
    status: "todo",
    priority: 1,
    due: "2026-04-22",
    tags: [],
  },
];

const VIEW_FIELDS = ["title", "status", "priority", "due", "tags"];

const VIEWS: Record<ViewType, BaseView> = {
  table: {
    name: "All tasks",
    type: "table",
    fields: VIEW_FIELDS,
    sort: [{ field: "priority", direction: "desc" }],
    filter: [],
  },
  kanban: {
    name: "By status",
    type: "kanban",
    fields: ["title", "priority", "due"],
    sort: [{ field: "priority", direction: "desc" }],
    filter: [],
    groupField: "status",
  },
  calendar: {
    name: "By due date",
    type: "calendar",
    fields: ["title", "status"],
    sort: [],
    filter: [{ field: "status", operator: "neq", value: "done" }],
    dateField: "due",
  },
  gallery: {
    name: "Cards",
    type: "gallery",
    fields: VIEW_FIELDS,
    sort: [{ field: "priority", direction: "desc" }],
    filter: [],
  },
};

const ORDER: ViewType[] = ["table", "kanban", "calendar", "gallery"];

export function BaseViewDemo() {
  const [active, setActive] = useState<ViewType>("table");
  const [records, setRecords] = useState<BaseRecord[]>(RECORDS);
  const view = VIEWS[active];

  const onRecordChange = useCallback(
    (id: string, patch: Record<string, unknown>) => {
      setRecords((prev) =>
        prev.map((r) => (r.id === id ? { ...r, ...patch } : r)),
      );
    },
    [],
  );

  return (
    <div className="base-view-demo">
      <nav className="base-view-tabs">
        {ORDER.map((t) => (
          <button
            key={t}
            type="button"
            className={t === active ? "tab active" : "tab"}
            onClick={() => setActive(t)}
          >
            {t}
          </button>
        ))}
      </nav>
      <div className="base-view-body">
        <BaseViewPanel
          records={records}
          schema={SCHEMA}
          view={view}
          onRecordChange={onRecordChange}
        />
      </div>
    </div>
  );
}
