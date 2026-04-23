// Helpers translating between the shell's view-mode enum and the
// `.bases` TOML `ViewType` enum (PRD-10 / nexus_types::bases). All
// six view modes (`table | kanban | calendar | gallery | list |
// timeline`) now round-trip — `List` + `Timeline` landed in the
// kernel wire schema alongside `BaseView.endField`.

import type { BaseView } from './kernelClient'
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
 *  zoom, collapsed groups, etc.) are intentionally dropped. */
export function viewFromTabState(
  name: string,
  mode: PersistableMode,
  tab: BasesTabState,
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
  return view
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
