// Helpers translating between the shell's view-mode enum and the
// `.bases` TOML `ViewType` enum (PRD-10 / nexus_types::bases). The
// kernel persists only `table | kanban | calendar | gallery`;
// `list` and `timeline` are shell-only and can't be saved as named
// views until that enum grows.

import type { BaseView } from './kernelClient'
import type { BasesTabState, ViewMode } from './basesStore'

export type PersistableMode = 'table' | 'board' | 'calendar' | 'gallery'

export function isPersistableMode(m: ViewMode): m is PersistableMode {
  return m === 'table' || m === 'board' || m === 'calendar' || m === 'gallery'
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
  if (mode === 'calendar' && tab.calendarDateField) {
    view.dateField = tab.calendarDateField
  }
  return view
}

/** Derive the patches needed to reproduce `view` on a fresh tab. */
export interface AppliedView {
  mode: ViewMode
  sort: { field: string; dir: 'asc' | 'desc' } | null
  boardGroupField: string | null
  calendarDateField: string | null
}

export function applyView(view: BaseView): AppliedView {
  const mode = modeFromViewType(view.type)
  const firstSort = view.sort?.[0]
  const dir = firstSort?.direction?.toLowerCase() === 'desc' ? 'desc' : 'asc'
  return {
    mode,
    sort: firstSort ? { field: firstSort.field, dir } : null,
    boardGroupField: mode === 'board' ? view.groupField ?? null : null,
    calendarDateField: mode === 'calendar' ? view.dateField ?? null : null,
  }
}
