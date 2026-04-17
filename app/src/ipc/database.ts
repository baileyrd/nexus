// Typed wrappers for the database core plugin Tauri commands.
//
// Thin adapters over `com.nexus.database` via kernel IPC (see
// `crates/nexus-app/src/database.rs`). Match the shape of
// `ipc/editor.ts` / `ipc/terminal.ts`.

import { invoke } from "@tauri-apps/api/core";

/** A single record in a `.bases` database. Matches
 *  `nexus_types::bases::BaseRecord` — `id` plus a free-form field bag. */
export interface BaseRecord {
  id: string;
  [field: string]: unknown;
}

/** Schema descriptor; `fields` is an opaque JSON map the engine
 *  currently only reads for future type-aware filters. */
export interface BaseSchema {
  version: string;
  fields: Record<string, unknown>;
}

/** View display type. Must match
 *  `nexus_types::bases::ViewType` (lowercase-serialized). */
export type ViewType = "table" | "kanban" | "calendar" | "gallery";

export interface SortRule {
  field: string;
  direction?: "asc" | "desc";
}

export interface FilterRule {
  field: string;
  operator: string;
  value: unknown;
}

/** A configured view over a base. Mirrors
 *  `nexus_types::bases::BaseView`. */
export interface BaseView {
  name: string;
  type: ViewType;
  fields?: string[];
  sort?: SortRule[];
  filter?: FilterRule[];
  /** Only for kanban views — the field to group columns by. */
  groupField?: string;
  /** Only for calendar views — the date field to bucket by day. */
  dateField?: string;
}

/** One group in a kanban / calendar layout. */
export interface ViewGroup {
  key: string;
  records: BaseRecord[];
}

/** Flat layout (Table / Gallery). */
interface FlatLayout {
  kind: "flat";
  records: BaseRecord[];
}

/** Grouped layout (Kanban / Calendar). */
interface GroupedLayout {
  kind: "grouped";
  groups: ViewGroup[];
}

export type ViewLayout = FlatLayout | GroupedLayout;

/** Response from `apply_view` — records filtered, sorted, and (for
 *  kanban / calendar) grouped according to the view's rules. */
export interface AppliedView {
  view_name: string;
  view_type: ViewType;
  fields: string[];
  layout: ViewLayout;
}

/** Sentinel key the engine uses for records missing the grouping
 *  field. Exposed so UI code can render it explicitly (e.g. greyed-out
 *  column on a kanban). */
export const MISSING_GROUP_KEY = "(none)";

/**
 * Apply a view to a record set. Returns the engine's `AppliedView`
 * with the records already filtered, sorted, and — for grouped view
 * types — bucketed.
 */
export function applyView(
  records: BaseRecord[],
  schema: BaseSchema,
  view: BaseView,
): Promise<AppliedView> {
  return invoke<AppliedView>("db_apply_view", { records, schema, view });
}

/** A full base loaded from a `.bases` directory on disk. Mirrors
 *  `nexus_types::bases::Base` — only the fields the UI currently
 *  reads are typed; the rest pass through as opaque JSON. */
export interface LoadedBase {
  name: string;
  schema: BaseSchema;
  records: BaseRecord[];
  views: BaseView[];
  relations?: unknown[];
  metadata?: unknown;
}

/** Load a `.bases` directory at `relpath` (forge-relative) into a
 *  full [`LoadedBase`]. Read-only — does not touch the SQLite index. */
export function loadBase(relpath: string): Promise<LoadedBase> {
  return invoke<LoadedBase>("load_forge_base", { relpath });
}
