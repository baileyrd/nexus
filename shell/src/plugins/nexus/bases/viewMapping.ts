// Helpers translating between the shell's view-mode enum and the
// `.bases` TOML `ViewType` enum (PRD-10 / nexus_types::bases). All
// six view modes (`table | kanban | calendar | gallery | list |
// timeline`) now round-trip — `List` + `Timeline` landed in the
// kernel wire schema alongside `BaseView.endField`.

import type { BaseView, FilterRule } from './kernelClient'
import type { BasesTabState, ViewMode } from './basesStore'

export type PersistableMode = ViewMode

export function isPersistableMode(m: ViewMode): m is PersistableMode {
  // Every ViewMode now has a ViewType counterpart.
  return true
}

/** Map wire ViewType → shell ViewMode. Unknown types fall back to
 *  `table` rather than erroring; the user can still rename/delete a
 *  corrupt view that way. */
export function modeFromViewType(type: BaseView['type']): ViewMode {
  switch (type) {
    case 'kanban':
      return 'board'
    case 'calendar':
      return 'calendar'
    case 'gallery':
      return 'gallery'
    case 'list':
      return 'list'
    case 'timeline':
      return 'timeline'
    case 'table':
    default:
      return 'table'
  }
}

export function viewTypeFromMode(m: PersistableMode): BaseView['type'] {
  switch (m) {
    case 'board':
      return 'kanban'
    case 'calendar':
      return 'calendar'
    case 'gallery':
      return 'gallery'
    case 'list':
      return 'list'
    case 'timeline':
      return 'timeline'
    case 'table':
      return 'table'
  }
}

/** Snapshot the bits of the current tab state that belong on a
 *  named view. Fields not expressible in the wire schema (shell-only
 *  zoom, collapsed groups, etc.) are intentionally dropped.
 *
 *  Phase 5 round-trip fix: now persists `fields` (the visible-column
 *  allowlist derived from `hiddenFields` + `allFields`) and `filter`
 *  (per-view filter chips). Pre-fix the snapshot dropped both, so a
 *  save→reload silently lost the user's column hides + filter chips. */
export function viewFromTabState(
  name: string,
  mode: PersistableMode,
  tab: BasesTabState,
  allFields: string[] = [],
): BaseView {
  const view: BaseView = {
    name,
    type: viewTypeFromMode(mode),
  }
  if (tab.sort) {
    view.sort = [{ field: tab.sort.field, direction: tab.sort.dir }]
  }
  if (mode === 'board' && tab.boardGroupField) {
    view.groupField = tab.boardGroupField
  }
  if (mode === 'list' && tab.listGroupField) {
    view.groupField = tab.listGroupField
  }
  if (mode === 'calendar' && tab.calendarDateField) {
    view.dateField = tab.calendarDateField
  }
  if (mode === 'timeline') {
    if (tab.timelineGroupField) view.groupField = tab.timelineGroupField
    if (tab.timelineStartField) view.dateField = tab.timelineStartField
    if (tab.timelineEndField) view.endField = tab.timelineEndField
  }
  // `fields` is an allowlist (kernel ViewType semantics): non-empty
  // means "render only these in this order"; empty/undefined means
  // "render every schema field". We materialise the visible subset
  // from `hiddenFields` so the saved view encodes the user's intent
  // even when columns are added later (a freshly-added column is
  // visible in views without a saved `fields` set, and hidden in
  // views with one — same as Notion / Airtable).
  if (tab.hiddenFields && tab.hiddenFields.length > 0 && allFields.length > 0) {
    const hidden = new Set(tab.hiddenFields)
    const visible = allFields.filter((f) => !hidden.has(f))
    if (visible.length > 0 && visible.length < allFields.length) {
      view.fields = visible
    }
  }
  if (tab.viewFilters.length > 0) {
    // Defensive copy so subsequent store mutations don't bleed into
    // the wire payload.
    view.filter = tab.viewFilters.map((f) => ({ ...f }))
  }
  return view
}

/** Reverse of `viewFromTabState` for hidden-fields: derive the
 *  hidden-field list from a view's `fields` allowlist + the schema's
 *  full field list. Empty/undefined `view.fields` means "no hides". */
export function hiddenFieldsFromView(
  view: BaseView,
  allFields: string[],
): string[] | null {
  if (!view.fields || view.fields.length === 0) return null
  const visible = new Set(view.fields)
  const hidden = allFields.filter((f) => !visible.has(f))
  return hidden.length > 0 ? hidden : null
}

/** Reverse of `viewFromTabState` for filters: defensive-copy the
 *  filter rules off the view so callers can mutate freely. */
export function filtersFromView(view: BaseView): FilterRule[] {
  return (view.filter ?? []).map((f) => ({ ...f }))
}

/** Derive the patches needed to reproduce `view` on a fresh tab. */
export interface AppliedView {
  mode: ViewMode
  sort: { field: string; dir: 'asc' | 'desc' } | null
  boardGroupField: string | null
  listGroupField: string | null
  calendarDateField: string | null
  timelineGroupField: string | null
  timelineStartField: string | null
  timelineEndField: string | null
}

export function applyView(view: BaseView): AppliedView {
  const mode = modeFromViewType(view.type)
  const firstSort = view.sort?.[0]
  const dir = firstSort?.direction?.toLowerCase() === 'desc' ? 'desc' : 'asc'
  return {
    mode,
    sort: firstSort ? { field: firstSort.field, dir } : null,
    boardGroupField: mode === 'board' ? view.groupField ?? null : null,
    listGroupField: mode === 'list' ? view.groupField ?? null : null,
    calendarDateField: mode === 'calendar' ? view.dateField ?? null : null,
    timelineGroupField: mode === 'timeline' ? view.groupField ?? null : null,
    timelineStartField: mode === 'timeline' ? view.dateField ?? null : null,
    timelineEndField: mode === 'timeline' ? view.endField ?? null : null,
  }
}
